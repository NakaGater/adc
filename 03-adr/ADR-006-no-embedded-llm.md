# ADR-006: ADCはLLM非搭載。エージェントは常にMCPクライアント側に置く

**Status:** Accepted

## Context

生成→検証→修復ループにはLLMが必要だが、利用形態は組織・個人で異なる: (1) Amazon Bedrock経由(企業のIAM/SSO統制・監査・コスト帰属)、(2) ユーザー個人のClaudeサブスクリプション(Pro/Max、モデル選択はユーザー次第)。ADC本体にLLMクライアントを埋め込むと、認証方式・課金・モデルIDの差異を全てRust側で抽象化する羽目になり、APIキー管理という不要なセキュリティ面積も抱え込む。

## Decision

1. **ADCは決定的なCLI+MCPサーバーに徹し、LLM APIを一切呼ばない。** RustコードベースにLLMクライアント・APIキー・プロンプトを持ち込まない(CI依存グラフチェックの対象に含める)
2. **修復ループのオーケストレーターは常にMCPクライアント側**(Claude Code / Claude Agent SDK)。ADCはツール(design_read / design_patch / build_and_check / evidence_query / narrow_param)として呼ばれる
3. **プロバイダ選択はユーザー環境の責務:**
   - サブスク: Claude CodeにPro/Maxでログインし、ADCのMCPサーバーを接続。モデル選択は`/model`
   - Bedrock: `CLAUDE_CODE_USE_BEDROCK=1` + AWS認証情報(標準クレデンシャルチェーン: SSOプロファイル/IAMロール/環境変数)。Agent SDKも同一の環境変数で切り替わり、エージェントコードは不変
4. **無人実行(CI/夜間探索)はAgent SDK+Bedrock(IAMベース)を標準とする。** 監査ログとコスト帰属の要件からサブスク認証はCI用途に使わない
5. **修復に必要な情報は全てADCの出力契約で保証する**(ADR-003のEvidence粒度、構造化エラー)。「LLMが賢いから伝わるだろう」に依存した省略をしない

## Consequences

- (+) 認証・課金・モデル更改への追従がゼロ。ADCのリリースサイクルがLLMエコシステムから独立する
- (+) セキュリティ面積の縮小: ADCは秘密情報を保持しない
- (+) Claude以外のMCP対応エージェントからも原理的に利用可能
- (−) 修復ループのプロンプト/スキル設計はADCリポジトリ外(配布用プラグイン/Skillとして別管理)になる。Phase 2でエージェント側の推奨Skill(`adc-repair`)を同梱ディレクトリ `agent-skills/` に置くことで緩和
- (−) ループ全体のE2Eテストは実LLMを要するため非決定的。CI必須テストからは除外し、成功基準4の検証は手動/定期実行とする
