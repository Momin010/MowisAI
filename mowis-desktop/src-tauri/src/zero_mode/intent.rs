// zero_mode/intent.rs — Intent classifier for zero mode
//
// Decides whether a user message should trigger orchestration (Build)
// or just get a chat response (Chat).

/// Whether the user wants to build something or just chat.
#[derive(Debug, Clone, PartialEq)]
pub enum UserIntent {
    Chat,
    Build,
}

/// Classify a user message as Chat or Build.
pub fn classify_intent(message: &str) -> UserIntent {
    let lower = message.to_lowercase();

    // ── Hard Chat overrides ──────────────────────────────────────────────────
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
    let strong_build: &[&str] = &[
        "create a ", "create an ", "create the ",
        "build a ", "build an ", "build the ",
        "make a ", "make an ", "make the ",
        "implement ", "write a ", "write an ", "write the ",
        "develop a ", "develop an ", "develop the ",
        "generate ", "scaffold ",
        "code me", "code a ", "code an ", "code the ",
        "can you code", "can you make", "can you build", "can you create",
        "can you write", "can you develop", "can you implement",
        "can you generate", "can you set up",
        "add ", "remove ", "delete ", "rename ",
        "refactor ", "rewrite ", "redesign ", "restructure ",
        "update ", "upgrade ", "migrate ", "port ",
        "fix ", "patch ", "resolve ", "debug ",
        "move ", "extract ", "split ", "merge ",
        "replace ", "convert ", "transform ",
        "set up ", "setup ", "configure ", "install ",
        "initialize ", "init ", "bootstrap ",
        "new feature", "add feature", "add support for",
        "integrate ", "connect ", "hook up",
        "wire up", "plug in",
    ];

    // ── Weak Build signals (each worth 1 point) ──────────────────────────────
    let weak_build: &[&str] = &[
        "website", "web app", "webapp", "landing page", "landing-page",
        "dashboard", "admin panel", "portfolio", "blog", "e-commerce",
        "mobile app", "cli tool", "rest api", "graphql", "crud",
        "microservice", "chatbot", "plugin", "extension", "script",
        "api", "endpoint", "route", "controller", "service",
        "database", "schema", "migration", "model",
        "component", "module", "function", "class",
        "test", "tests", "spec", "auth", "authentication",
        "login", "signup", "register", "session", "jwt",
        "ui", "form", "button", "page", "layout",
        "deploy", "dockerfile", "ci/cd", "pipeline",
        "for my ", "for our ", "for the ",
        "my app", "my project", "my codebase", "my code", "my company",
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

    // Hard chat override
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
