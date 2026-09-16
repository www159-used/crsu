use crate::crucible::{Client, User};

pub(crate) struct Candidates {
    pub(crate) token: String,
    pub(crate) projects: Vec<String>,
    pub(crate) repositories: Vec<String>,
    pub(crate) reviewers: Vec<User>,
}

/// Performs the Crucible conversation required before the form can select defaults.
pub(crate) fn load_candidates(
    url: &str,
    username: &str,
    password: &str,
) -> Result<Candidates, String> {
    let token = Client::login(url, username, password).map_err(|error| error.to_string())?;
    let client = Client::new(url, token.clone());
    let projects = client.project_keys().map_err(|error| error.to_string())?;
    if projects.is_empty() {
        return Err("Crucible returned no projects".to_owned());
    }
    let repositories = client
        .repository_names()
        .map_err(|error| error.to_string())?;
    let reviewers = client
        .users()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|user| user.username != username)
        .collect();
    Ok(Candidates {
        token,
        projects,
        repositories,
        reviewers,
    })
}
