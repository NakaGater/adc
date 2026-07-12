//! adc-mcp — ADCのMCPサーバー (M6-1, US-28, ADR-006)。
//!
//! usage: adc-mcp --design <path> [--gated]
//!
//! - stdio transportのみ(外部通信ゼロ — 06-deployment.md)
//! - 1設計に束縛。ツール引数にパスは持たせない
//! - --gated: design_patchを全Pass時のみ適用(無人・自動適用の安全装置。
//!   対話セッションは非gatedが既定 — 人間のPRレビューがゲート)

use adc_mcp::{AdcCore, Edit};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router, ServerHandler, ServiceExt,
};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EmptyArgs {}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EditArg {
    #[schemars(description = "置換対象(設計ソース内で一意に一致すること。0件/複数件はE-PATCH)")]
    pub old_string: String,
    pub new_string: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PatchArgs {
    #[schemars(description = "design_readで得た現行ソースのsha256(楽観ロック)")]
    pub base_sha256: String,
    #[schemars(description = "完全一致の文字列置換列(full_sourceとどちらか一方)")]
    pub edits: Option<Vec<EditArg>>,
    #[schemars(description = "全置換ソース(editsとどちらか一方)")]
    pub full_source: Option<String>,
    #[schemars(description = "trueなら検証(+gated check)まで行い書き込まない")]
    pub dry_run: Option<bool>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CheckArgs {
    #[schemars(description = "trueで片端Fail軸の二分探索(suggested_range)も返す")]
    pub narrow: Option<bool>,
    #[schemars(description = "assert_idで結果を絞る")]
    pub filter: Option<Vec<String>>,
    pub no_cache: Option<bool>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EvidenceArgs {
    pub assert_id: Option<String>,
    #[schemars(description = "pass | fail | inconclusive")]
    pub status: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NarrowArgs {
    #[schemars(description = "対象のOpenパラメータid(省略時は全片端Fail軸)")]
    pub param: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ExplainArgs {
    #[schemars(description = "param/part/anchor/feature/assertion等のid(種別横断検索)")]
    pub id: String,
}

pub struct AdcServer {
    core: AdcCore,
    #[allow(dead_code)] // tool_handlerマクロが参照するフィールド
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl AdcServer {
    pub fn new(core: AdcCore) -> Self {
        Self {
            core,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "正典design.ronのソース・sha256(楽観ロック用)・静的検証結果を返す")]
    fn design_read(&self, Parameters(EmptyArgs {}): Parameters<EmptyArgs>) -> String {
        self.core.design_read().to_string()
    }

    #[tool(
        description = "正典へのパッチ適用。editsは一意一致必須(0件/複数件はE-PATCH)。base_sha256の楽観ロック付き。gatedサーバーでは全Pass時のみ書き込み"
    )]
    fn design_patch(&self, Parameters(a): Parameters<PatchArgs>) -> String {
        let edits = a.edits.map(|es| {
            es.into_iter()
                .map(|e| Edit {
                    old_string: e.old_string,
                    new_string: e.new_string,
                })
                .collect()
        });
        self.core
            .design_patch(&a.base_sha256, edits, a.full_source, a.dry_run.unwrap_or(false))
            .to_string()
    }

    #[tool(
        description = "コンパイル+全検証。exit_code(0=全Pass/1=Fail/2=Inconclusive)とCheckResult列(Open含みは3点評価samples付き)、残自由度を返す"
    )]
    fn build_and_check(&self, Parameters(a): Parameters<CheckArgs>) -> String {
        self.core
            .build_and_check(a.narrow.unwrap_or(false), a.filter, a.no_cache.unwrap_or(false))
            .to_string()
    }

    #[tool(description = "検証結果をassert_id/statusで絞って返す(キャッシュ付き再検証+フィルタ)")]
    fn evidence_query(&self, Parameters(a): Parameters<EvidenceArgs>) -> String {
        self.core
            .evidence_query(a.assert_id.as_deref(), a.status.as_deref())
            .to_string()
    }

    #[tool(
        description = "Openパラメータの実行可能区間絞り込み(片端Failのみ、二分探索8回)。構造化suggested_rangeを返す"
    )]
    fn narrow_param(&self, Parameters(a): Parameters<NarrowArgs>) -> String {
        self.core.narrow_param(a.param.as_deref()).to_string()
    }

    #[tool(
        description = "idの定義・rationale連鎖・参照元(referenced_by/related)。編集前の影響調査に必ず使うこと。板金partは展開長(derived)も返す"
    )]
    fn explain(&self, Parameters(a): Parameters<ExplainArgs>) -> String {
        self.core.explain(&a.id).to_string()
    }
}

#[tool_handler]
impl ServerHandler for AdcServer {
    fn get_info(&self) -> ServerInfo {
        let gate = if self.core.gated {
            "gated(patchは全Pass時のみ適用 — 無人・自動適用の安全装置)"
        } else {
            "非gated(対話セッション既定 — 人間のPRレビューがゲート)"
        };
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            format!(
                "ADC (AI-native Declarative CAD) サーバー。対象: {}。モード: {}。\n\
                 正典はdesign.ron。編集前にexplainで影響調査(referenced_by)を行うこと。\n\
                 エラーはE-*の構造化JSONで返る(修復ループの入力)。",
                self.core.design_path.display(),
                gate
            ),
        )
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut design: Option<String> = None;
    let mut gated = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--design" => {
                design = args.get(i + 1).cloned();
                i += 2;
            }
            "--gated" => {
                gated = true;
                i += 1;
            }
            "-h" | "--help" => {
                print!("Usage: adc-mcp --design <path> [--gated]\n\nADCのMCPサーバー(stdio)。--gatedはdesign_patchを全Pass時のみ適用(無人運転の安全装置)。\n");
                return Ok(());
            }
            "-V" | "--version" => {
                println!("adc-mcp {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            other => {
                eprintln!("adc-mcp: 不明な引数: {other}");
                std::process::exit(2);
            }
        }
    }
    let Some(design) = design else {
        eprintln!("usage: adc-mcp --design <path> [--gated]");
        std::process::exit(2);
    };

    let server = AdcServer::new(AdcCore::new(design, gated));
    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
