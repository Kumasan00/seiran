//! PDFテキスト生成アプリケーション
//!
//! このアプリケーションは、テキストファイルを読み込み、
//! 指定されたフォントを使用してPDFドキュメントを生成します。
//! フォントのサブセット化、テキストシェーピング、グリフマッピングを処理します。

use std::{collections::HashMap, fs, io};

use font::FontContext;
use pdf_writer::{Content, Finish, Name, Str, types};
use read_config_file::Config;
use stypes::GlyphMapping;

// 定数

/// .notdef グリフのグリフID
const NOTDEF_GID: u16 = 0;
/// 行の高さの倍率
const LINE_HEIGHT_FACTOR: f32 = 1.0;
/// CID to GIDマッピングのレジストリ名
const CID_TO_GID_REGISTRY: &[u8] = b"Kuma";
/// CID to GIDマッピングのオーダリング名
const CID_TO_GID_ORDERING: &[u8] = b"Custom";
/// CID to GIDマッピングのサプリメント番号
const CID_TO_GID_SUPPLEMENT: i32 = 0;

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
  let arg = cli::parse_arg()?;
  let config = read_config_file::read_config_file()?;

  println!("Config loaded: {:?}", config);

  let lines = read_file::read_file(&arg.file_path)?;

  let mut font_ctx = FontContext::new(
    config
      .main_font
      .font_path
      .to_str()
      .ok_or("Invalid UTF-8 in font path")?,
    &config,
  )?;

  let mut mapping = GlyphMapping::new();

  let content = process_text_lines(lines, &mut font_ctx, &mut mapping, &config)?;
  let subset_bytes = font::create_font_subset(&font_ctx, &mapping)?;
  println!("Subset font: {} bytes", subset_bytes.len());

  let font_info = font::analyze_subset_font(&subset_bytes, font_ctx.index)?;
  mapping.advance_widths.insert(NOTDEF_GID, font_info.upem);

  let advance_list = mapping.build_advance_list(font_info.upem);
  let cid_to_gid_map = mapping.build_cid_to_gid_map();
  let to_unicode_cmap = create_to_unicode_cmap(&config.main_font.font_name, mapping.cid_to_chars);

  pdf_gen::pdf_gen(
    &subset_bytes,
    &font_info,
    &advance_list,
    &cid_to_gid_map,
    to_unicode_cmap,
    content,
    &config,
  )?;

  println!("PDF generated");
  Ok(())
}

/// テキストの各行を処理してPDFコンテンツストリームを生成
///
/// # 引数
///
/// * `lines` - 処理するテキスト行のイテレータ
/// * `font_ctx` - フォントコンテキスト
/// * `mapping` - グリフマッピング情報
/// * `config` - アプリケーション設定
///
/// # 戻り値
///
/// PDFコンテンツストリームを返します。
///
/// # エラー
///
/// 行の処理中にエラーが発生した場合にエラーを返します。
fn process_text_lines(
  lines: io::Lines<io::BufReader<fs::File>>,
  font_ctx: &mut FontContext,
  mapping: &mut GlyphMapping,
  config: &Config,
) -> Result<Content, Box<dyn std::error::Error>> {
  let upem = font_ctx.ttf_face.units_per_em() as f32;
  let mut content = Content::new();
  content.begin_text();
  content.set_font(
    Name(config.main_font.font_name.as_bytes()),
    config.pdf.font_size,
  );
  content.next_line(
    config.pdf.margin_left,
    config.pdf.height - config.pdf.margin_top,
  );

  for (line_num, line) in lines.enumerate() {
    let line = line?;
    println!("Line {}: {}", line_num + 1, line);

    process_single_line(&line, font_ctx, upem, mapping, &mut content)?;

    content.next_line(0.0, -config.pdf.font_size * LINE_HEIGHT_FACTOR);
  }

  content.end_text();
  Ok(content)
}

/// 単一行のテキストを処理してコンテンツストリームに追加
///
/// テキストシェーピングを実行し、グリフIDとCIDのマッピングを更新し、
/// 位置調整を行いながらコンテンツストリームに書き込みます。
///
/// # 引数
///
/// * `line` - 処理するテキスト行
/// * `font_ctx` - フォントコンテキスト
/// * `upem` - フォントのユニット/em値
/// * `mapping` - グリフマッピング情報
/// * `content` - PDFコンテンツストリーム
///
/// # エラー
///
/// シェーピングまたはコンテンツ書き込み中にエラーが発生した場合にエラーを返します。
fn process_single_line(
  line: &str,
  font_ctx: &mut FontContext,
  upem: f32,
  mapping: &mut GlyphMapping,
  content: &mut Content,
) -> Result<(), Box<dyn std::error::Error>> {
  let mut position_text = content.show_positioned();
  let mut items = position_text.items();

  let shape_results = text::shaping(
    line,
    &mut font_ctx.hb_font,
    &mut mapping.gid_to_cid,
    &mut mapping.used_gids,
  );

  let mut text_buffer = Vec::new();

  for (j, shape_result) in shape_results.iter().enumerate() {
    let gid = shape_result.gid;
    let cid = *mapping.gid_to_cid.get(&gid).unwrap();

    let advance_width = font_ctx.get_glyph_advance(gid) * 1000.0 / upem; // 1000/upem スケーリング
    mapping.advance_widths.entry(cid).or_insert(advance_width);
    let shape_advance = shape_result.x_advance as f32 * 1000.0 / upem; // 1000/upem スケーリング

    // CIDをバイト列に変換
    text_buffer.push((cid >> 8) as u8);
    text_buffer.push((cid & 0xFF) as u8);

    // 位置調整が必要な場合
    let advance_diff = advance_width - shape_advance;
    if advance_diff != 0.0 {
      items.show(Str(&text_buffer));
      items.adjust(advance_diff);
      text_buffer.clear();
    }

    // Unicode マッピングを記録
    let char_range = get_char_range(line, &shape_results, j);
    let chars: Vec<char> = line[char_range].chars().collect();
    mapping.cid_to_chars.insert(cid, chars);
  }

  items.show(Str(&text_buffer));
  items.finish();
  position_text.finish();

  Ok(())
}

/// シェーピング結果から文字範囲を取得
///
/// 指定されたインデックスのシェーピング結果に対応する
/// 元のテキストの文字範囲を返します。
///
/// # 引数
///
/// * `line` - 元のテキスト行
/// * `shape_results` - シェーピング結果のスライス
/// * `current_index` - 現在のシェーピング結果のインデックス
///
/// # 戻り値
///
/// 文字範囲を表す`Range<usize>`を返します。
fn get_char_range(
  line: &str,
  shape_results: &[text::ShapingResult],
  current_index: usize,
) -> std::ops::Range<usize> {
  let start = shape_results[current_index].cluster as usize;
  let end = shape_results
    .get(current_index + 1)
    .map(|sr| sr.cluster as usize)
    .unwrap_or(line.len());
  start..end
}

/// ToUnicode CMapを作成
///
/// CIDから対応するUnicode文字へのマッピングを持つ
/// CMapオブジェクトを生成します。
///
/// # 引数
///
/// * `font_name` - フォント名
/// * `cid_to_chars` - CIDと文字のマッピング
///
/// # 戻り値
///
/// Unicode CMapを返します。
fn create_to_unicode_cmap(
  font_name: &str,
  cid_to_chars: HashMap<u16, Vec<char>>,
) -> types::UnicodeCmap {
  let system_info = types::SystemInfo {
    registry: Str(CID_TO_GID_REGISTRY),
    ordering: Str(CID_TO_GID_ORDERING),
    supplement: CID_TO_GID_SUPPLEMENT,
  };

  let name = format!("{}_ToUnicode", font_name);
  let mut cmap = types::UnicodeCmap::new(Name(name.as_bytes()), system_info);

  for (cid, chars) in cid_to_chars {
    cmap.pair_with_multiple(cid, chars.into_iter());
  }

  cmap
}
