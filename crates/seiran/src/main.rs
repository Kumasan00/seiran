//! PDFテキスト生成アプリケーション
//!
//! このアプリケーションは、テキストファイルを読み込み、
//! 指定されたフォントを使用してPDFドキュメントを生成します。
//! フォントのサブセット化、テキストシェーピング、グリフマッピングを処理します。

mod build_pdf;
mod subcommand;
use tracing::info;
use tracing_subscriber::fmt;

/// アプリケーションのメインエントリーポイント
///
/// 以下の処理を実行します：
/// 1. コマンドライン引数の解析
/// 2. 設定ファイルの読み込み
/// 3. 入力テキストファイルの読み込み
/// 4. フォントの初期化とテキスト処理
/// 5. フォントサブセットの作成
/// 6. PDF生成
///
/// # エラー
///
/// ファイルI/O、フォント処理、PDF生成のいずれかで問題が発生した場合にエラーを返します。
fn main() -> Result<(), Box<dyn std::error::Error>> {
  fmt::fmt()
    .with_max_level(tracing::Level::INFO)
    .with_thread_ids(false)
    .with_thread_names(false)
    .with_target(false)
    .with_timer(fmt::time::LocalTime::rfc_3339())
    .init();

  let cli_args = cli::parse_arg();

  match cli_args.command {
    cli::Command::Build { text_file_path } => {
      let absolute_path = text_file_path.canonicalize()?;
      info!(absolute_path = %absolute_path.display(), "Input text file path");
      build_pdf::build_pdf(&absolute_path)?;
    },
    cli::Command::VariationAxes { font_path } => {
      let absolute_path = font_path.canonicalize()?;
      subcommand::get_variation_axes(&absolute_path)?;
    },
    cli::Command::TtcNames { ttc_file_path } => {
      let absolute_path = ttc_file_path.canonicalize()?;
      subcommand::get_ttc_names(&absolute_path)?;
    },
    cli::Command::ScriptLangs { font_path } => {
      let absolute_path = font_path.canonicalize()?;
      subcommand::get_script_langs(&absolute_path)?;
    },
  }

  Ok(())
}
