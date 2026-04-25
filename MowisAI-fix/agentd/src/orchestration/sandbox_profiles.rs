//! Pre-defined package sets per team type (Alpine packages where possible).

/// Base packages for a sandbox team profile. Planner may add `required_packages` on top.
pub fn get_packages_for_team(team_type: &str) -> Vec<String> {
    let t = team_type.to_lowercase();
    match t.as_str() {
        "frontend-team" | "frontend" => vec![
            "nodejs".to_string(),
            "npm".to_string(),
            "git".to_string(),
            "curl".to_string(),
        ],
        "backend-team" | "backend" => vec![
            "python3".to_string(),
            "py3-pip".to_string(),
            "nodejs".to_string(),
            "npm".to_string(),
            "git".to_string(),
            "curl".to_string(),
        ],
        "devops-team" | "devops" => vec![
            "docker".to_string(),
            "git".to_string(),
            "curl".to_string(),
            "python3".to_string(),
        ],
        "testing-team" | "testing" => vec![
            "python3".to_string(),
            "py3-pip".to_string(),
            "nodejs".to_string(),
            "npm".to_string(),
            "git".to_string(),
        ],
        "data-team" | "data" => vec![
            "python3".to_string(),
            "py3-pip".to_string(),
            "git".to_string(),
            "curl".to_string(),
        ],
        "security-team" | "security" => vec![
            "nmap".to_string(),
            "curl".to_string(),
            "git".to_string(),
            "python3".to_string(),
        ],
        "general" | "general-team" => vec![
            "git".to_string(),
            "curl".to_string(),
            "python3".to_string(),
            "nodejs".to_string(),
            "npm".to_string(),
        ],
        _ => get_packages_for_team("general"),
    }
}

/// Merge profile packages with task-specific extras, deduplicated, stable order.
pub fn merge_packages(team_type: &str, extra: &[String]) -> Vec<String> {
    let mut out: Vec<String> = get_packages_for_team(team_type);
    for p in extra {
        let p = p.trim();
        if !p.is_empty() && !out.iter().any(|x| x == p) {
            out.push(p.to_string());
        }
    }
    out
}
