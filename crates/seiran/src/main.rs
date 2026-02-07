//! PDFテキスト生成アプリケーション
//!
//! このアプリケーションは、テキストファイルを読み込み、
//! 指定されたフォントを使用してPDFドキュメントを生成します。
//! フォントのサブセット化、テキストシェーピング、グリフマッピングを処理します。

mod build_pdf;
mod subcommand;
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
fn main() -> miette::Result<()> {
  fmt::fmt()
    .with_max_level(tracing::Level::INFO)
    .with_thread_ids(false)
    .with_thread_names(false)
    .with_target(false)
    .with_timer(fmt::time::LocalTime::rfc_3339())
    .init();

  let cli_args = cli::parse_arg();

  match cli_args.command {
    cli::Command::Build { text_file_path } => match build_pdf::build_pdf(&text_file_path) {
      Ok(()) => {},
      Err(e) => {
        eprintln!("{e:?}");
        std::process::exit(1);
      },
    },
    cli::Command::VariationAxes {
      font_path,
      font_index,
    } => {
      subcommand::get_variation_axes(&font_path, font_index)?;
    },
    cli::Command::TtcNames { ttc_file_path } => {
      subcommand::get_ttc_names(&ttc_file_path)?;
    },
    cli::Command::ScriptLangs {
      font_path,
      font_index,
    } => {
      subcommand::script_langs(&font_path, font_index)?;
    },
  }

  Ok(())
}
