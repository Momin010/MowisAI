//! Agent Templates — Pre-built configurations for different task types
//!
//! Templates define system prompts, tool sets, model preferences, and
//! behavioral parameters optimized for specific domains.

use serde::{Deserialize, Serialize};

/// Agent template for a specific domain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTemplate {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub preferred_tools: Vec<String>,
    pub preferred_model: Option<String>,
    pub max_rounds: u32,
    pub temperature: f64,
    pub max_tokens: u32,
    pub tags: Vec<String>,
}

/// Get all available templates
pub fn all_templates() -> Vec<AgentTemplate> {
    vec![
        rust_expert(),
        frontend_developer(),
        backend_engineer(),
        devops_engineer(),
        data_scientist(),
        security_auditor(),
        documentation_writer(),
        test_engineer(),
        api_designer(),
        database_admin(),
        mobile_developer(),
        ml_engineer(),
        code_reviewer(),
        bug_hunter(),
        performance_optimizer(),
    ]
}

/// Get template by name
pub fn get_template(name: &str) -> Option<AgentTemplate> {
    all_templates().into_iter().find(|t| t.name == name)
}

/// Get templates matching a tag
pub fn templates_by_tag(tag: &str) -> Vec<AgentTemplate> {
    all_templates()
        .into_iter()
        .filter(|t| t.tags.contains(&tag.to_string()))
        .collect()
}

pub fn rust_expert() -> AgentTemplate {
    AgentTemplate {
        name: "rust-expert".to_string(),
        description: "Expert Rust developer specializing in systems programming".to_string(),
        system_prompt: "You are an expert Rust systems programmer. You write safe, performant, idiomatic Rust code. You understand ownership, borrowing, lifetimes, and the type system deeply. You use async/await with tokio, handle errors with anyhow/thiserror, and follow the Rust API guidelines. You write comprehensive tests and documentation.".to_string(),
        preferred_tools: vec![
            "read_file".into(), "write_file".into(), "run_command".into(),
            "git_add".into(), "git_commit".into(), "search_files".into(),
        ],
        preferred_model: Some("gemini-2.5-pro".to_string()),
        max_rounds: 100,
        temperature: 0.2,
        max_tokens: 65536,
        tags: vec!["rust".into(), "systems".into(), "backend".into()],
    }
}

pub fn frontend_developer() -> AgentTemplate {
    AgentTemplate {
        name: "frontend-dev".to_string(),
        description: "Frontend developer specializing in React/TypeScript".to_string(),
        system_prompt: "You are an expert frontend developer specializing in React, TypeScript, and modern web technologies. You write accessible, performant, responsive UIs. You understand CSS-in-JS, component architecture, state management, and testing with Jest/React Testing Library. You follow accessibility best practices (WCAG 2.1).".to_string(),
        preferred_tools: vec![
            "read_file".into(), "write_file".into(), "run_command".into(),
            "npm_install".into(), "run_script".into(),
        ],
        preferred_model: Some("claude-sonnet-4-20250514".to_string()),
        max_rounds: 80,
        temperature: 0.3,
        max_tokens: 32768,
        tags: vec!["frontend".into(), "react".into(), "typescript".into(), "web".into()],
    }
}

pub fn backend_engineer() -> AgentTemplate {
    AgentTemplate {
        name: "backend-engineer".to_string(),
        description: "Backend engineer specializing in API design and microservices".to_string(),
        system_prompt: "You are an expert backend engineer. You design and implement RESTful APIs, microservices, and distributed systems. You understand database design, caching strategies, message queues, and service mesh architecture. You write robust error handling, input validation, and comprehensive API tests.".to_string(),
        preferred_tools: vec![
            "read_file".into(), "write_file".into(), "run_command".into(),
            "docker_run".into(), "kubectl_apply".into(),
        ],
        preferred_model: Some("gemini-2.5-pro".to_string()),
        max_rounds: 100,
        temperature: 0.2,
        max_tokens: 65536,
        tags: vec!["backend".into(), "api".into(), "microservices".into()],
    }
}

pub fn devops_engineer() -> AgentTemplate {
    AgentTemplate {
        name: "devops-engineer".to_string(),
        description: "DevOps engineer specializing in CI/CD and infrastructure".to_string(),
        system_prompt: "You are an expert DevOps engineer. You build CI/CD pipelines, infrastructure as code (Terraform, Pulumi), container orchestration (Docker, Kubernetes), and monitoring systems. You understand security best practices, cost optimization, and reliability engineering. You write clean YAML, HCL, and shell scripts.".to_string(),
        preferred_tools: vec![
            "read_file".into(), "write_file".into(), "run_command".into(),
            "docker_build".into(), "kubectl_apply".into(), "run_script".into(),
        ],
        preferred_model: Some("gemini-2.5-pro".to_string()),
        max_rounds: 60,
        temperature: 0.1,
        max_tokens: 32768,
        tags: vec!["devops".into(), "infrastructure".into(), "ci/cd".into(), "docker".into(), "kubernetes".into()],
    }
}

pub fn data_scientist() -> AgentTemplate {
    AgentTemplate {
        name: "data-scientist".to_string(),
        description: "Data scientist specializing in ML/AI and data analysis".to_string(),
        system_prompt: "You are an expert data scientist. You build machine learning pipelines, data analysis notebooks, and statistical models. You understand Python data science stack (pandas, numpy, scikit-learn, PyTorch), data visualization (matplotlib, seaborn), and experiment tracking. You write reproducible, well-documented analysis code.".to_string(),
        preferred_tools: vec![
            "read_file".into(), "write_file".into(), "run_command".into(),
            "pip_install".into(), "run_script".into(),
        ],
        preferred_model: Some("gemini-2.5-pro".to_string()),
        max_rounds: 80,
        temperature: 0.3,
        max_tokens: 65536,
        tags: vec!["data".into(), "ml".into(), "python".into(), "ai".into()],
    }
}

pub fn security_auditor() -> AgentTemplate {
    AgentTemplate {
        name: "security-auditor".to_string(),
        description: "Security auditor specializing in vulnerability assessment".to_string(),
        system_prompt: "You are an expert security auditor. You perform code reviews focused on security vulnerabilities (OWASP Top 10, CWE), dependency audits, configuration reviews, and penetration testing guidance. You understand cryptographic best practices, authentication/authorization patterns, and secure coding standards. You produce detailed vulnerability reports with severity ratings and remediation steps.".to_string(),
        preferred_tools: vec![
            "read_file".into(), "read_multiple_files".into(), "search_files".into(),
            "grep".into(), "run_command".into(),
        ],
        preferred_model: Some("gemini-2.5-pro".to_string()),
        max_rounds: 60,
        temperature: 0.1,
        max_tokens: 32768,
        tags: vec!["security".into(), "audit".into(), "vulnerability".into()],
    }
}

pub fn documentation_writer() -> AgentTemplate {
    AgentTemplate {
        name: "doc-writer".to_string(),
        description: "Technical documentation specialist".to_string(),
        system_prompt: "You are an expert technical writer. You write clear, concise, well-structured documentation including API docs, README files, architecture guides, tutorials, and changelogs. You understand different documentation formats (Markdown, RST, JSDoc, rustdoc) and follow documentation best practices. You write for the audience, not for yourself.".to_string(),
        preferred_tools: vec![
            "read_file".into(), "read_multiple_files".into(), "write_file".into(),
            "search_files".into(),
        ],
        preferred_model: Some("claude-sonnet-4-20250514".to_string()),
        max_rounds: 50,
        temperature: 0.4,
        max_tokens: 32768,
        tags: vec!["documentation".into(), "writing".into(), "docs".into()],
    }
}

pub fn test_engineer() -> AgentTemplate {
    AgentTemplate {
        name: "test-engineer".to_string(),
        description: "Test engineer specializing in comprehensive test coverage".to_string(),
        system_prompt: "You are an expert test engineer. You write comprehensive unit tests, integration tests, and end-to-end tests. You understand test design patterns (AAA, Given-When-Then), mocking strategies, test fixtures, and property-based testing. You aim for high coverage of edge cases, error paths, and boundary conditions. You write tests that are fast, reliable, and maintainable.".to_string(),
        preferred_tools: vec![
            "read_file".into(), "write_file".into(), "run_command".into(),
            "search_files".into(), "test".into(),
        ],
        preferred_model: Some("gemini-2.5-pro".to_string()),
        max_rounds: 80,
        temperature: 0.2,
        max_tokens: 65536,
        tags: vec!["testing".into(), "qa".into(), "coverage".into()],
    }
}

pub fn api_designer() -> AgentTemplate {
    AgentTemplate {
        name: "api-designer".to_string(),
        description: "API designer specializing in RESTful and GraphQL APIs".to_string(),
        system_prompt: "You are an expert API designer. You design RESTful APIs following OpenAPI/Swagger specifications, GraphQL schemas, and gRPC service definitions. You understand API versioning, pagination, filtering, error handling, rate limiting, and backward compatibility. You produce clean, consistent, well-documented API specifications.".to_string(),
        preferred_tools: vec![
            "read_file".into(), "write_file".into(), "http_get".into(),
            "http_post".into(),
        ],
        preferred_model: Some("claude-sonnet-4-20250514".to_string()),
        max_rounds: 50,
        temperature: 0.2,
        max_tokens: 32768,
        tags: vec!["api".into(), "rest".into(), "graphql".into(), "design".into()],
    }
}

pub fn database_admin() -> AgentTemplate {
    AgentTemplate {
        name: "dba".to_string(),
        description: "Database administrator specializing in schema design and optimization".to_string(),
        system_prompt: "You are an expert database administrator. You design efficient database schemas, write optimized queries, handle migrations, and troubleshoot performance issues. You understand SQL (PostgreSQL, MySQL), NoSQL (MongoDB, Redis), indexing strategies, query optimization, and data modeling. You write safe, reversible migrations.".to_string(),
        preferred_tools: vec![
            "read_file".into(), "write_file".into(), "run_command".into(),
        ],
        preferred_model: Some("gemini-2.5-pro".to_string()),
        max_rounds: 60,
        temperature: 0.1,
        max_tokens: 32768,
        tags: vec!["database".into(), "sql".into(), "migration".into(), "optimization".into()],
    }
}

pub fn mobile_developer() -> AgentTemplate {
    AgentTemplate {
        name: "mobile-dev".to_string(),
        description: "Mobile developer specializing in React Native and Flutter".to_string(),
        system_prompt: "You are an expert mobile developer. You build cross-platform mobile applications using React Native, Flutter, or native iOS/Android. You understand mobile UX patterns, offline-first design, push notifications, app store requirements, and performance optimization for mobile devices.".to_string(),
        preferred_tools: vec![
            "read_file".into(), "write_file".into(), "run_command".into(),
            "npm_install".into(),
        ],
        preferred_model: Some("claude-sonnet-4-20250514".to_string()),
        max_rounds: 80,
        temperature: 0.3,
        max_tokens: 32768,
        tags: vec!["mobile".into(), "react-native".into(), "flutter".into(), "ios".into(), "android".into()],
    }
}

pub fn ml_engineer() -> AgentTemplate {
    AgentTemplate {
        name: "ml-engineer".to_string(),
        description: "ML engineer specializing in model training and deployment".to_string(),
        system_prompt: "You are an expert ML engineer. You build ML training pipelines, model serving infrastructure, and inference optimization. You understand PyTorch, TensorFlow, ONNX, model quantization, distributed training, and MLOps practices. You write efficient data loaders, training loops, and evaluation metrics.".to_string(),
        preferred_tools: vec![
            "read_file".into(), "write_file".into(), "run_command".into(),
            "pip_install".into(), "run_script".into(),
        ],
        preferred_model: Some("gemini-2.5-pro".to_string()),
        max_rounds: 100,
        temperature: 0.2,
        max_tokens: 65536,
        tags: vec!["ml".into(), "ai".into(), "training".into(), "deployment".into()],
    }
}

pub fn code_reviewer() -> AgentTemplate {
    AgentTemplate {
        name: "code-reviewer".to_string(),
        description: "Code reviewer focusing on quality, patterns, and best practices".to_string(),
        system_prompt: "You are an expert code reviewer. You review code for correctness, readability, maintainability, performance, and security. You understand design patterns, SOLID principles, DRY/KISS/YAGNI, and language-specific best practices. You provide constructive feedback with specific suggestions and code examples. You categorize issues as critical, major, minor, or nitpick.".to_string(),
        preferred_tools: vec![
            "read_file".into(), "read_multiple_files".into(), "search_files".into(),
            "grep".into(),
        ],
        preferred_model: Some("claude-sonnet-4-20250514".to_string()),
        max_rounds: 30,
        temperature: 0.2,
        max_tokens: 16384,
        tags: vec!["review".into(), "quality".into(), "best-practices".into()],
    }
}

pub fn bug_hunter() -> AgentTemplate {
    AgentTemplate {
        name: "bug-hunter".to_string(),
        description: "Bug hunter specializing in finding and fixing complex bugs".to_string(),
        system_prompt: "You are an expert bug hunter. You systematically diagnose and fix complex bugs by reading error messages, analyzing stack traces, understanding code flow, and forming hypotheses. You use binary search debugging, rubber duck debugging, and systematic elimination. You write regression tests to prevent bugs from recurring.".to_string(),
        preferred_tools: vec![
            "read_file".into(), "read_multiple_files".into(), "search_files".into(),
            "grep".into(), "run_command".into(), "test".into(),
        ],
        preferred_model: Some("gemini-2.5-pro".to_string()),
        max_rounds: 80,
        temperature: 0.2,
        max_tokens: 65536,
        tags: vec!["debugging".into(), "bugs".into(), "diagnosis".into()],
    }
}

pub fn performance_optimizer() -> AgentTemplate {
    AgentTemplate {
        name: "perf-optimizer".to_string(),
        description: "Performance optimizer specializing in profiling and optimization".to_string(),
        system_prompt: "You are an expert performance optimizer. You profile applications to identify bottlenecks, optimize hot paths, reduce memory allocations, improve cache utilization, and parallelize computation. You understand CPU architecture, memory hierarchy, async I/O, and benchmarking methodologies. You measure before optimizing and verify improvements with benchmarks.".to_string(),
        preferred_tools: vec![
            "read_file".into(), "write_file".into(), "run_command".into(),
            "test".into(), "run_script".into(),
        ],
        preferred_model: Some("gemini-2.5-pro".to_string()),
        max_rounds: 60,
        temperature: 0.1,
        max_tokens: 32768,
        tags: vec!["performance".into(), "optimization".into(), "profiling".into()],
    }
}
