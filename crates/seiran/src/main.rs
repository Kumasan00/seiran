//! PDF テキスト生成アプリケーション（Seiran）
//!
//! Seiran は、TeX スタイルのテキストファイルから高品質な PDF を生成する
//! コマンドラインアプリケーションです。
//!
//! ## 主要機能
//!
//! - **19 フォント種別対応**: Serif/Sans Serif/Monospace（各 4 種）+ Math + 日本語（6 種）
//! - **高度なテキストシェイピング**: `HarfRust` による OpenType フィーチャー対応
//! - **フォント最適化**: 使用グリフのみをサブセット化して PDF 出力（krilla が内部実施）
//! - **多言語対応**: Latin、日本語、その他の言語を自動判定
//! - **バリアブルフォント**: 可変軸の指定とインスタンス化
//!
//! ## 実行フロー
//!
//! 通常の PDF 生成フロー（`build` サブコマンド）：
//! 1. CLI パース → TOML 設定読み込み（config / style / references）
//! 2. 字句解析・構文解析 → 評価（Document IR）→ lowering（`LayoutNode`）
//! 3. フォント読み込み・検証 → シェーピング + 計測（`layout::build_blocks`）
//! 4. 画像サイズ確定（`pdf_gen::resolve_images`）→ 行分割・縦組版（`hlist::break_pages`）
//! 5. PDF 描画（`pdf_gen::create_pdf`）→ ファイル出力
//!
//! ## サブコマンド
//!
//! - `build [-c <config>]` - 設定ファイルの `sources` 配列から PDF を生成（メイン機能）
//! - `variation-axes <font>` - フォントのバリアブル軸情報を表示
//! - `ttc-names <ttc_file>` - TTC ファイル内のフォント名一覧を表示
//! - `script-langs <font>` - フォント対応の Script/Language タグを表示
//!
//! ## ロギング
//!
//! INFO レベル以上のトレーシングログを出力します。
//! タイムスタンプ付きで標準エラー出力に記録されます。

mod build_pdf;
use tracing_subscriber::fmt;

/// アプリケーションのメインエントリーポイント
///
/// ロギング（INFO 以上、RFC 3339 タイムスタンプ付きで stderr）を初期化し、
/// CLI 引数で指定されたサブコマンドを実行します。
///
/// # Errors
///
/// 設定読み込み・フォント検証・パース・PDF 生成の各段のエラーを
/// `miette` 診断として表示します。
fn main() -> miette::Result<()> {
  fmt::fmt()
    .pretty()
    .with_thread_ids(false)
    .with_thread_names(false)
    .with_target(false)
    .with_file(false)
    .with_timer(fmt::time::LocalTime::rfc_3339())
    .init();

  let cli_args = cli::parse_arg();

  match cli_args.command {
    cli::Command::Build { config_path } => build_pdf::build_pdf(&config_path)?,
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

  return Ok(());
}
