/// Detect whether a user message is a "build" request (needs orchestration)
/// or a "chat" request (direct Gemini response).
///
/// This is a lightweight heuristic check. For now, we look for action keywords.
/// In the future, this could use an LLM classifier.
pub fn classify_intent(message: &str) -> UserIntent {
    let lower = message.to_lowercase();

    // Strong build signals
    let build_keywords = [
        "build", "create", "implement", "develop", "write the code",
        "generate", "scaffold", "set up", "setup", "refactor",
        "add feature", "new feature", "migrate", "port",
        "rewrite", "redesign", "restructure",
    ];

    // Strong chat signals (override build keywords if present)
    let chat_keywords = [
        "explain", "what is", "how does", "why", "tell me",
        "describe", "help me understand", "what are",
        "compare", "difference between", "opinion",
        "plan", "design", "think about", "should i",
    ];

    let has_build = build_keywords.iter().any(|k| lower.contains(k));
    let has_chat = chat_keywords.iter().any(|k| lower.contains(k));

    if has_build && !has_chat {
        UserIntent::Build
    } else {
        UserIntent::Chat
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum UserIntent {
    Chat,
    Build,
}
