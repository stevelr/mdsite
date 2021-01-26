//! github client library for fetching content from Github
//!
use crate::{Error, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::json;

const GITHUB_ENDPOINT: &str = "https://api.github.com";
const GH_USER_AGENT: &str = "mdsite";

/// Response from Github list-tree
#[derive(Debug, Deserialize)]
pub struct GithubTree {
    // sha: String
    // url: String
    pub tree: Vec<GithubTreeItem>,
    pub truncated: bool,
}

/// file item from Github list-tree
#[derive(Debug, Deserialize)]
pub struct GithubTreeItem {
    pub path: String,
    // mode: String,
    // type: String (tree | blob | ...)
    pub sha: String,
    // url: String
}

/// Response from get-content queries
#[derive(Debug, Deserialize)]
struct ContentResponse {
    size: u64,
    sha: String,
    content: String,
    encoding: String,
}

/// A person in github api (author or committer)
pub struct Person {
    /// Person's name
    pub name: String,
    /// Person's email
    pub email: String,
}

/// Parameters for commit request
pub struct Commit<'params> {
    /// path to content within repo
    pub path: &'params str,
    /// raw Content
    pub bytes: &'params Vec<u8>,
    /// branch name
    pub branch: &'params str,
    /// sha of previous commit
    pub prev_sha: &'params str,
    /// Commit message
    pub message: &'params str,
    /// Name of committer to be written to commit log
    pub committer_name: &'params str,
    /// Email of committer to be written to commit log
    pub committer_email: &'params str,
}

/// a portion of the commit-content response containing fields we care about
#[derive(Deserialize)]
struct WithSha {
    sha: String,
}
/// a portion of the commit-content response containing fields we care about
#[derive(Deserialize)]
struct CommitResp {
    content: WithSha,
    commit: WithSha,
}

/// Github Api client
pub struct Github {
    /// repository name
    repo: String,
    /// repository owner
    owner: String,
    /// github personal api token
    api_token: String,
}

impl Github {
    pub fn init<T: Into<String>>(repo: T, owner: T, api_token: T) -> Self {
        Github {
            repo: repo.into(),
            owner: owner.into(),
            api_token: api_token.into(),
        }
    }

    /// List objects at HEAD of specified branch that match predicate
    pub async fn list_content<P>(&self, branch: &str, predicate: P) -> Result<Vec<GithubTreeItem>>
    where
        P: Fn(&GithubTreeItem) -> bool,
    {
        let url = format!(
            "{endpoint}/repos/{owner}/{repo}/git/trees/{branch}?recursive=1",
            endpoint = GITHUB_ENDPOINT,
            owner = &self.owner,
            repo = &self.repo,
            branch = branch
        );
        let resp: GithubTree = self.get(&url).await?;

        // just get paths for content items - in the proper folder and ending with ".md"
        let tree = resp.tree.into_iter().filter(predicate).collect();
        Ok(tree)
    }

    /// Retrieve object by path and branch HEAD. Returns content and blob sha
    pub async fn get_content_by_path(
        &self,
        content_path: &str,
        branch: &str,
    ) -> Result<(Vec<u8>, String)> {
        let url = format!(
            "{endpoint}/repos/{owner}/{repo}/contents/{content_path}/?ref={branch}",
            endpoint = GITHUB_ENDPOINT,
            owner = &self.owner,
            repo = &self.repo,
            content_path = content_path,
            branch = branch, // ref
        );
        let resp: ContentResponse = self.get(&url).await?;
        let bytes = decode_content(&url, &resp)?;
        Ok((bytes, resp.sha))
    }

    /// Retrieves github content by its SHA id
    pub async fn get_content_by_sha(&self, blob_id: &str) -> Result<Vec<u8>> {
        let url = format!(
            "{endpoint}/repos/{owner}/{repo}/git/blobs/{blob_id}",
            endpoint = GITHUB_ENDPOINT,
            owner = &self.owner,
            repo = &self.repo,
            blob_id = blob_id
        );

        let resp: ContentResponse = self.get(&url).await?;
        let bytes = decode_content(&url, &resp)?;
        Ok(bytes)
    }

    /// Commit content. Result is (content-sha, commit-sha)
    pub async fn commit(&self, params: &Commit<'_>) -> Result<(String, String)> {
        let url = format!(
            "{}/repos/{owner}/{repo}/contents/{path}",
            GITHUB_ENDPOINT,
            owner = &self.owner,
            repo = &self.repo,
            path = params.path
        );

        let body = json!({
            "message": params.message,
            "content": base64::encode(params.bytes),
            "sha": params.prev_sha,
            "branch": params.branch,
            "committer" : {
                "name": params.committer_name,
                "email": params.committer_email,
            }
        });
        let resp: CommitResp = self.put(&url, &body).await?;

        Ok((resp.content.sha, resp.commit.sha))
    }

    /// Performs http GET on github url and returns deserialized object
    async fn get<Resp: DeserializeOwned>(&self, url: &str) -> Result<Resp> {
        let obj = self.request(url, reqwest::Client::new().get(url)).await?;
        Ok(obj)
    }

    /// Performs http PUT on github url and returns deserialized object
    async fn put<Req: Serialize, Resp: DeserializeOwned>(
        &self,
        url: &str,
        body: &Req,
    ) -> Result<Resp> {
        let obj = self
            .request(url, reqwest::Client::new().put(url).json(body))
            .await?;
        Ok(obj)
    }

    /// complete request object and deserialize result, with error handling
    async fn request<Resp: DeserializeOwned>(
        &self,
        url: &str,
        req: reqwest::RequestBuilder,
    ) -> Result<Resp> {
        let obj = req
            .header("Accept", "application/vnd.github.v3+json")
            .header("Authorization", format!("token {}", self.api_token))
            .header("User-Agent", GH_USER_AGENT)
            .send()
            .await
            .map_err(|e| Error::Github(url.to_string(), e.to_string()))?
            .error_for_status()
            .map_err(|e| Error::Github(url.to_string(), e.to_string()))?
            .json()
            .await
            .map_err(|e| Error::Github(url.to_string(), e.to_string()))?;
        Ok(obj)
    }
}

/// Remove newlines from the string. The reason for this is that Github content blobs are
/// base64 encoded, but the text has embedded newlines, which the base64 crate rejects,
fn remove_newlines(s: &str) -> String {
    s.chars().filter(|&c| c != '\n').collect()
}

/// base64 decode content blob
fn decode_content(url: &str, resp: &ContentResponse) -> Result<Vec<u8>> {
    if &resp.encoding != "base64" {
        return Err(Error::Github(
            url.into(),
            format!("expected base64 encoding, got '{}'", &resp.encoding),
        ));
    }
    let content = base64::decode(remove_newlines(&resp.content))
        .map_err(|e| Error::Base64(url.into(), e.to_string()))?;
    Ok(content)
}
