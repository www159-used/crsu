use crate::crucible::{Client, RepositoryCandidate, User};

pub(crate) struct Candidates {
    pub(crate) token: String,
    pub(crate) projects: Vec<String>,
    pub(crate) repositories: Vec<RepositoryCandidate>,
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
    // Crucible-only installs often report isFishEye=false but still expose
    // repositories-v1; gate on the repo list itself, not the FishEye flag.
    let repositories = client.repositories().map_err(|error| error.to_string())?;
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

pub(crate) fn detected_repository<'a>(
    remote_url: &str,
    repositories: &'a [RepositoryCandidate],
) -> Option<&'a RepositoryCandidate> {
    if remote_url.trim().is_empty() {
        return None;
    }
    let mut matches = repositories.iter().filter(|repository| {
        repository.scm_type.eq_ignore_ascii_case("git")
            && crate::git_repository::git_remotes_match(&repository.location, remote_url)
    });
    let recommendation = matches.next()?;
    matches.next().is_none().then_some(recommendation)
}
