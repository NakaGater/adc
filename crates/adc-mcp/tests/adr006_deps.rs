//! ADR-006 依存グラフチェック: LLMクライアント・HTTPクライアントの持ち込み禁止。
//!
//! 06-deployment.md「外部通信ゼロ」の静的検知。Cargo.lockの全パッケージ名を
//! 禁止リストと照合する(設計メモ m6-1 §4 — 禁止対象の明文化)。

/// 禁止クレート(完全一致)。HTTPクライアント+LLM SDK。
/// rmcpのHTTP系はfeatureで無効化している(transport-ioのみ)ため、
/// 依存グラフに現れた時点で構成ミス
const DENY: &[&str] = &[
    // HTTPクライアント
    "reqwest",
    "ureq",
    "isahc",
    "surf",
    "attohttpc",
    // LLMクライアントSDK
    "async-openai",
    "openai",
    "openai-api-rs",
    "anthropic",
    "anthropic-sdk",
    "genai",
    "ollama-rs",
];

#[test]
fn cargo_lock_has_no_llm_or_http_clients() {
    let lock = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock"))
        .expect("Cargo.lock");
    let mut hits = Vec::new();
    for line in lock.lines() {
        if let Some(name) = line.strip_prefix("name = \"").and_then(|s| s.strip_suffix('"')) {
            if DENY.contains(&name) {
                hits.push(name.to_string());
            }
        }
    }
    assert!(
        hits.is_empty(),
        "ADR-006違反: 禁止クレートが依存グラフに存在: {hits:?}(featureの見直し、または代替を検討)"
    );
}
