fn main() {
  // Call the CLI function
  let arg = match cli::parse_single_arg() {
    Ok(val) => val,
    Err(e) => {
      eprintln!("エラー: {e}");
      std::process::exit(1);
    }
  };
  let font_path = "/Users/takumu/rust/pdftest/NotoSansJP-Regular.ttf";
  // Call the file reading function
  let text: Vec<String> = read_file::read_file(&arg)
    .expect("ファイルを読み込めません。")
    .collect();
  let used_glyphs =
    font::usedglyph::usedflyph(font_path, &text).expect("使われているグリフを取得できません。");

  println!("使われているグリフの数: {}", used_glyphs.len());
  println!("使われているグリフ: {:?}", used_glyphs);

  // Call the PDF generation function
  pdf_gen::pdf_gen(&text).expect("pdf が生成できません。");
}
