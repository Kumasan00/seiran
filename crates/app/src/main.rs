//! PDFテキスト生成アプリケーション
//!
//! このアプリケーションは、テキストファイルを読み込み、
//! 指定されたフォントを使用してPDFドキュメントを生成します。
//! フォントのサブセット化、テキストシェーピング、グリフマッピングを処理します。

#[allow(unused_imports)]
use std::{
  fs,
  fs::File,
  io::{self, BufRead, BufReader},
  path::Path,
  result,
};

#[allow(unused_imports)]
use font::{
  self,
  font_context::{self, FontContexts},
  font_data,
};
use ttf_parser::Face;
#[allow(unused_imports)]
use types::GlyphMappings;

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
  let cli_args = cli::parse_arg();

  match cli_args.command {
    cli::Command::Build { text_file_path } => {
      let absolute_path = text_file_path.canonicalize()?;
      println!("Building PDF from: {:?}", absolute_path);
      build_pdf(&absolute_path)?;
    },
    cli::Command::TtcNames { ttc_file_path } => {
      let absolute_path = ttc_file_path.canonicalize()?;
      get_ttc_names(&absolute_path)?;
    },
  }

  Ok(())
}

/// PDFを生成する
///
/// 設定ファイルとテキストファイルを読み込み、フォント処理を行い、
/// PDFドキュメントを生成します。
///
/// # 引数
///
/// * `file_path` - 入力テキストファイルのパス
///
/// # 戻り値
///
/// 成功した場合は`Ok(())`を返します。
///
/// # エラー
///
/// ファイル読み込み、フォント処理、PDF生成のいずれかで失敗した場合。
fn build_pdf<P: AsRef<Path>>(file_path: P) -> Result<(), Box<dyn std::error::Error>> {
  let config = read_config_file::read_config_file()?;
  println!("{}", config.name);

  parser::text_parser(&file_path)?;

  // let text_lines = read_file(&file_path)?;
  // let mut font_contexts = FontContexts::new(&config)?;
  // let mut glyph_mappings = GlyphMappings::new();

  // let pdf_content =
  //   text::process_text_lines(text_lines, &mut font_contexts, &mut glyph_mappings, &config)?;

  // let subset_bytes = font_context::create_font_subset(&font_contexts, &glyph_mappings)?;

  // let font_datas = font_data::analyze_subset_font(&subset_bytes)?;

  // font::insert_notdef_advance_widths(&mut glyph_mappings, &font_datas);

  // pdf_gen::pdf_gen(
  //   &subset_bytes,
  //   &font_datas,
  //   &glyph_mappings,
  //   pdf_content,
  //   &config,
  // )?;

  // println!("PDF generated");
  Ok(())
}

/// TTCファイルから各フォントの名前情報を取得して表示
///
/// TrueTypeコレクション(TTC)ファイルに含まれる全てのフォントの
/// nameテーブル情報を標準出力に表示します。各フォントのインデックスと
/// プラットフォームID、Name ID、名前文字列を出力します。
///
/// # 引数
///
/// * `file_path` - TTCファイルのパス
///
/// # 戻り値
///
/// 成功した場合は`Ok(())`を返します。
///
/// # エラー
///
/// ファイルの読み込みまたはフォント解析に失敗した場合にエラーを返します。
fn get_ttc_names<P: AsRef<Path>>(file_path: P) -> result::Result<(), Box<dyn std::error::Error>> {
  let font_data = fs::read(file_path)?;
  let font_count = ttf_parser::fonts_in_collection(&font_data).unwrap();
  println!("Number of fonts in TTC: {}", font_count);
  for font_index in 0..font_count {
    println!("\nFont index: {}\n", font_index);
    let face = Face::parse(&font_data, font_index)?;
    // let name = font::extract_font_name(&face)?;
    let names = face.names();
    for name_entry in names {
      let platform_id = name_entry.platform_id;
      println!(
        "Platform ID {:?}: Name ID {}: {:?}",
        platform_id,
        name_entry.name_id,
        name_entry.to_string()
      );
    }
  }
  Ok(())
}
