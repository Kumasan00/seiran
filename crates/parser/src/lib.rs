//! CST から Document IR への評価
//!
//! このクレートは [`syntax`] クレートが生成した CST を走査し、
//! PDF 生成パイプラインで使用される Document IR（`DocNode`）を生成します。
//!
//! ## 処理パイプライン
//!
//! ```text
//! ソーステキスト
//!   ↓ [syntax::parse]  アリーナベース CST を構築
//! CST (syntax::green::GreenNode) — bumpalo::Bump アリーナ上
//!   ↓ [evaluator]       型付きビュー (CommandView, EnvironmentView) を介して
//!                        コマンド・環境を評価し Document IR に変換
//! Document IR (document::DocNode, document::InlineNode)
//! ```
//!
//! ## モジュール構成
//!
//! - [`evaluator`] — CST → Document IR の変換器（IR の型定義は `document` クレートに分離）

use std::collections::HashSet;

use bumpalo::Bump;
use document::DocNode;
use miette::{Diagnostic, NamedSource};
use thiserror::Error;
use tracing::debug;

mod evaluator;
pub use evaluator::EvalError;
use evaluator::cite::resolve_cites;

/// `parse_source` が返すエラー型
///
/// 構文解析（[`syntax::ParserError`]）と評価（[`EvalError`]）のいずれかをラップします。
/// 各バリアントは [`NamedSource`] を保持しており、内側のエラーが持つ `#[label]`
/// 情報と組み合わせて、`miette` のフル診断（ソースコード付き）が表示されます。
///
/// 内側のエラーが持つ診断属性（`code` / `help` / `#[label]`）は
/// `#[diagnostic_source]` により外側へ自動的に伝播されます。
#[derive(Debug, Error, Diagnostic)]
pub enum ParseSourceError {
  /// 構文解析（`syntax::parse`）で発生したエラー
  #[error("構文解析に失敗しました")]
  #[diagnostic(code(parser::parse_source::syntax))]
  Syntax {
    /// ソース名付きの元テキスト（`#[label]` をレンダリングするための `source_code`）
    #[source_code]
    src: NamedSource<String>,
    /// 元の構文エラー
    #[source]
    #[diagnostic_source]
    error: syntax::ParserError,
  },

  /// 評価（CST → Document IR 変換）で発生したエラー
  #[error("評価に失敗しました")]
  #[diagnostic(code(parser::parse_source::eval))]
  Eval {
    /// ソース名付きの元テキスト（`#[label]` をレンダリングするための `source_code`）
    #[source_code]
    src: NamedSource<String>,
    /// 元の評価エラー
    #[source]
    #[diagnostic_source]
    error: EvalError,
  },
}

/// ソーステキストをパースして Document IR（`Vec<DocNode>`）を生成する
///
/// I/O はこの関数の責務外です。呼び出し側でファイルを読み込み、
/// ソース文字列とソース名を渡してください。
///
/// bumpalo アリーナ上に CST を構築し、型付きビューを介して評価し、
/// 所有権を持つ `Vec<DocNode>` を返します。アリーナは関数終了時に一括解放されます。
/// 採番（見出し・図表・数式の自動採番とラベル解決）は行いません（`lowering` 層の責務）。
///
/// # Arguments
///
/// * `source` - パース対象のソーステキスト
/// * `source_name` - エラー表示用のソース名（ファイルパス等）
/// * `citation_keys` - 参照定義（references）の有効な参照 ID 集合。`\cite` のキー存在検証に使う
///
/// # Errors
///
/// パースまたは評価で失敗した場合に [`ParseSourceError`] を返します。
/// 返されるエラーには [`NamedSource`] が同梱されているため、呼び出し側で
/// `with_source_code` を追加する必要はありません。
// `ParseSourceError` は `EvalError` のフィールドが大きく ~168 バイトになるが、
// `parse_source` はソースファイルごとに 1 回しか呼ばれないため Result のサイズは
// 性能上の問題にならない。Box<dyn Diagnostic + Send + Sync> で型消去すると呼び出し側で
// 内側エラーの variant match ができなくなるため、具体型のまま返してこの lint を抑止する。
// `citation_keys` は呼び出し側が既定ハッシャで構築した集合をそのまま受けるため、
// BuildHasher を総称化せず `HashSet<String>` で受ける（implicit_hasher を許可）。
#[allow(clippy::result_large_err, clippy::implicit_hasher)]
pub fn parse_source(
  source: &str,
  source_name: &str,
  citation_keys: &HashSet<String>,
) -> Result<Vec<DocNode>, ParseSourceError> {
  let arena = Bump::new();
  let cst =
    syntax::parse(source, &arena, evaluator::lookup_env_parse_mode).map_err(|error| ParseSourceError::Syntax {
      src: NamedSource::new(source_name, source.to_string()),
      error,
    })?;

  let doc_nodes = evaluator::evaluate_children(source, cst).map_err(|error| ParseSourceError::Eval {
    src: NamedSource::new(source_name, source.to_string()),
    error,
  })?;

  // pass2: `\cite{...}` の引用キーが references に存在するかを検証する（未定義は集約報告）
  resolve_cites(&doc_nodes, citation_keys).map_err(|error| ParseSourceError::Eval {
    src: NamedSource::new(source_name, source.to_string()),
    error,
  })?;

  debug!(source_path = source_name, node_count = doc_nodes.len(), "ソースのパース・評価が完了しました");
  return Ok(doc_nodes);
}
