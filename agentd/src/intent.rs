//! Intent classifier — decides whether a user message should trigger
//! orchestration (Build) or just get a chat response (Chat).
//!
//! ## Design
//!
//! Old approach: required the word "build" explicitly. Too restrictive.
//!
//! New approach: score-based. Count build signals and chat signals independently,
//! then decide by majority. Build wins on a tie when signals are equal and
//! there's at least one build signal — bias toward action.
//!
//! The classifier is deliberately liberal: it's much better to mistakenly
//! kick off orchestration on an ambiguous message than to silently answer
//! "add auth to my app" with a chat response. The user can always just ask
//! a follow-up question; orchestration that doesn't run is much worse.

/// Whether the user wants to build something or just chat.
#[derive(Debug, Clone, PartialEq)]
pub enum UserIntent {
    Chat,
    Build,
}

/// Classify a user message as Chat or Build.
///
/// Returns `Build` when the message looks like an action request for code
/// changes, and `Chat` for explanatory / conversational input.
pub fn classify_intent(message: &str) -> UserIntent {
    let lower = message.to_lowercase();

    // ── Hard Chat overrides ──────────────────────────────────────────────────
    // Pure question patterns — these are almost never build requests.
    let hard_chat_patterns: &[&str] = &[
        "what is ", "what are ", "what does ", "what's ",
        "how does ", "how do ", "how is ",
        "why does ", "why is ", "why are ", "why do ",
        "explain ", "can you explain",
        "tell me about", "tell me what",
        "describe ", "definition of",
        "difference between", "compare ",
        "what should i", "should i use", "which is better",
        "pros and cons", "advantages of", "disadvantages of",
    ];

    let is_hard_chat = hard_chat_patterns.iter().any(|p| lower.contains(p));

    // ── Strong Build signals (each worth 2 points) ───────────────────────────
    // These almost always mean "do the work in the codebase".
    let strong_build: &[&str] = &[
        // Creation
        "create a ", "create an ", "create the ",
        "build a ", "build an ", "build the ",
        "make a ", "make an ", "make the ",
        "implement ", "write a ", "write an ", "write the ",
        "develop a ", "develop an ", "develop the ",
        "generate ", "scaffold ",
        // Modification
        "add ", "remove ", "delete ", "rename ",
        "refactor ", "rewrite ", "redesign ", "restructure ",
        "update ", "upgrade ", "migrate ", "port ",
        "fix ", "patch ", "resolve ", "debug ",
        "move ", "extract ", "split ", "merge ",
        "replace ", "convert ", "transform ",
        // Setup
        "set up ", "setup ", "configure ", "install ",
        "initialize ", "init ", "bootstrap ",
        // Feature work
        "new feature", "add feature", "add support for",
        "integrate ", "connect ", "hook up",
        "wire up", "plug in",
    ];

    // ── Weak Build signals (each worth 1 point) ──────────────────────────────
    // Contextual — more likely build than chat but need other signals too.
    let weak_build: &[&str] = &[
        "api", "endpoint", "route", "controller", "service",
        "database", "schema", "migration", "model",
        "component", "module", "function", "class",
        "test", "tests", "spec", "auth", "authentication",
        "login", "signup", "register", "session", "jwt",
        "ui", "form", "button", "page", "layout",
        "deploy", "dockerfile", "ci/cd", "pipeline",
        "my app", "my project", "my codebase", "my code",
        "the app", "the project", "the codebase",
        "the backend", "the frontend", "the api",
    ];

    // ── Strong Chat signals (each worth 2 points) ────────────────────────────
    let strong_chat: &[&str] = &[
        "explain", "understand", "what is", "how does",
        "tell me", "describe", "clarify",
        "opinion", "think about", "advice", "recommend",
        "best practice", "should i", "is it possible",
        "can i", "would you", "could you explain",
        "help me understand",
    ];

    // ── Score ────────────────────────────────────────────────────────────────
    let build_score: u32 = strong_build.iter().filter(|k| lower.contains(*k)).count() as u32 * 2
        + weak_build.iter().filter(|k| lower.contains(*k)).count() as u32;

    let chat_score: u32 = strong_chat.iter().filter(|k| lower.contains(*k)).count() as u32 * 2;

    // Hard chat override — a pure question pattern wins unless there's a
    // strong build signal (score ≥ 2, meaning at least one strong_build keyword).
    // This catches "how does authentication work?" (weak build score from "auth")
    // but still routes "how do I add authentication to my app?" → Build
    // because "add" (strong_build, +2) pushes score above the threshold.
    if is_hard_chat && build_score < 2 {
        return UserIntent::Chat;
    }

    // Build wins when build_score >= chat_score AND there's at least one signal
    if build_score > 0 && build_score >= chat_score {
        UserIntent::Build
    } else if chat_score > build_score {
        UserIntent::Chat
    } else {
        // No signals at all → chat (safe default)
        UserIntent::Chat
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn b(msg: &str) {
        assert_eq!(
            classify_intent(msg),
            UserIntent::Build,
            "Expected Build for: {:?}",
            msg
        );
    }
    fn c(msg: &str) {
        assert_eq!(
            classify_intent(msg),
            UserIntent::Chat,
            "Expected Chat for: {:?}",
            msg
        );
    }

    // ── Should trigger Build ─────────────────────────────────────────────────
    #[test]
    fn test_build_explicit() {
        b("build a login page");
        b("build the auth module");
    }

    #[test]
    fn test_create_requests() {
        b("create a REST API for user management");
        b("create an auth system with JWT");
        b("make a signup form");
    }

    #[test]
    fn test_add_requests() {
        b("add authentication to my app");
        b("add a new endpoint for payments");
        b("add dark mode support");
    }

    #[test]
    fn test_fix_requests() {
        b("fix the login bug");
        b("fix the broken tests");
        b("fix the CSS layout");
    }

    #[test]
    fn test_implement_requests() {
        b("implement JWT auth");
        b("implement the checkout flow");
        b("implement pagination for the API");
    }

    #[test]
    fn test_refactor_requests() {
        b("refactor the database layer");
        b("refactor my components to TypeScript");
    }

    #[test]
    fn test_write_requests() {
        b("write a function that parses JSON");
        b("write tests for the auth module");
        b("write the database migration");
    }

    #[test]
    fn test_update_requests() {
        b("update the user model to include a phone field");
        b("upgrade the dependencies");
    }

    #[test]
    fn test_migrate_requests() {
        b("migrate the database to PostgreSQL");
        b("port the Python code to Rust");
    }

    #[test]
    fn test_setup_requests() {
        b("set up CI/CD pipeline");
        b("configure the Docker environment");
        b("initialize a new Express project");
    }

    #[test]
    fn test_make_requests() {
        b("make the navbar responsive");
        b("make a dashboard with charts");
    }

    #[test]
    fn test_no_explicit_build_word() {
        // These should all be Build even without "build"
        b("add user authentication with OAuth");
        b("create the product catalog page");
        b("fix the broken API endpoint");
        b("refactor everything to use async/await");
        b("integrate Stripe payments");
    }

    // ── Should trigger Chat ──────────────────────────────────────────────────
    #[test]
    fn test_pure_questions() {
        c("what is JWT?");
        c("how does OAuth work?");
        c("why is Rust fast?");
        c("what are the differences between REST and GraphQL?");
    }

    #[test]
    fn test_explain_requests() {
        c("explain how middleware works");
        c("can you explain the repository pattern?");
        c("explain the difference between SQL and NoSQL");
    }

    #[test]
    fn test_opinion_requests() {
        c("should I use Postgres or MySQL?");
        c("what's the best way to handle auth?");
        c("compare React and Vue");
    }

    #[test]
    fn test_hello() {
        c("hello");
        c("hi there");
        c("thanks");
        c("ok");
    }

    #[test]
    fn test_ambiguous_with_explanation() {
        // "how does" is a hard chat pattern; "auth" is only a weak build signal (+1).
        // build_score=1 < 2 → hard chat guard fires → Chat
        c("how does authentication work in Express?");
        // "how do I add" → hard chat + strong build ("add" = +2). build_score=2 ≥ threshold → Build
        b("how do I add authentication to my app?");
    }

    #[test]
    fn test_tell_me_vs_action() {
        c("tell me about JWT tokens");
        b("add JWT tokens to my app");
    }

    #[test]
    fn test_no_input() {
        c("");
        c("   ");
    }
}
