//! CLI のユーザー向け報告と開発者向け tracing 設定
//!
//! 出力を、warning 診断と成功サマリからなるユーザー向け報告と、処理観測用の tracing に分ける。
//! 呼び出し側は [`Reporter`] の初期化と報告操作だけを知る。
//! `trace` が進捗と詳細ログを各出力先へ振り分け、`log_file` がファイル書き込みの成否を保持する。端末側の出力先は stderr で、stdout はパイプできる成果物のための経路として空けておく。
//!
//! `--log-file` を指定したときは、端末の出力をそのままに**ログファイルを足す**。実行記録と終了記録は
//! tracing を通さないので、フィルタに依らず残る。
//!
//! 端末へ書けないとき（読み手が先に閉じたパイプ等）は、その報告を捨てて処理を続ける。終了コードとログファイルへの
//! 記録は、端末へ書けたかに依らない。診断を描けないとき（miette が `fmt::Error` を返す）は、本文と code だけの
//! 簡略表示に置き換える（[`render_diagnostic`]）。

mod log_file;
mod trace;

use std::{
  ffi::OsStr,
  fmt::{self, Write as _},
  io::{self, IsTerminal, Write},
  path::Path,
  time::Duration,
};

use log_file::LogSink;
pub(super) use log_file::{LogFailure, LogFileError};
use miette::{Diagnostic, GraphicalReportHandler, GraphicalTheme, MietteHandler, ReportHandler};
use time::format_description::well_known::Rfc3339;
use tracing_subscriber::fmt::{
  format::Writer,
  time::{FormatTime, OffsetTime},
};

use crate::cli::{Command, LogOptions};

/// CLI のユーザー向け報告器。
///
/// `quiet` の解釈と ANSI 装飾の可否を保持し、warning と成功サマリへ一貫して適用する。tracing subscriber は
/// [`Reporter::init`] でプロセス全体に 1 回だけ初期化する。
pub(super) struct Reporter {
  /// 端末への非エラー出力を抑止するか。
  quiet: bool,
  /// stderr へ ANSI 装飾を出してよいか。
  ansi: bool,
  /// ログファイルへの書き出し口（`--log-file` 指定時のみ）。
  log: Option<LogSink>,
  /// 端末へ warning 診断を描く miette の既定 handler（`Report` の `Debug` 表示が使うのと同じもの）。
  terminal: MietteHandler,
}

impl Reporter {
  /// tracing を初期化し、同じ quiet 方針と装飾方針を持つ報告器を返す。
  ///
  /// # Errors
  ///
  /// `--log-file` のパスを開けないとき、subscriber を 1 つも設置せずに [`LogFileError`] を返す。
  pub(super) fn init(options: LogOptions, header: &RunHeader<'_>) -> Result<Self, LogFileError> {
    let ansi = ansi_enabled(std::env::var_os("NO_COLOR").as_deref(), io::stderr().is_terminal());
    let log = options.log_file.as_deref().map(LogSink::open).transpose()?;
    let quiet = options.quiet;
    let warning = trace::install(options, ansi, log.as_ref(), header);

    let reporter = Reporter {
      quiet,
      ansi,
      log,
      terminal: MietteHandler::new(),
    };
    if let Some(warning) = &warning {
      reporter.warning(warning);
    }
    return Ok(reporter);
  }

  /// コンパイルが返した warning 診断を報告する。
  pub(super) fn warnings(&self, warnings: &seiran_compiler::Warnings) {
    for warning in warnings {
      self.warning(warning);
    }
  }

  /// warning 診断 1 件を報告する。
  ///
  /// 端末へは致命的エラー（`Report` の `Debug` 表示）と同じ体裁で描く（[`render_diagnostic`]）。
  /// ログファイルへは `--quiet` でも省かない。
  fn warning(&self, diagnostic: &dyn Diagnostic) {
    if !self.quiet {
      write_terminal(&render_diagnostic(&self.terminal, diagnostic));
    }
    if let Some(log) = &self.log {
      log.write_block(&render_diagnostic_plain(diagnostic));
    }
  }

  /// ビルド成功時のサマリを報告する。
  ///
  /// 時間は compiler だけでなく render と保存を含む CLI の build 全体。完了記号を着色するかは
  /// [`Reporter::init`] が決めた 1 つの判定に従うため、ログの装飾と食い違わない。
  pub(super) fn build(&self, compilation: &seiran_compiler::Compilation, elapsed: Duration) {
    let page_count = compilation.publication.pages().len();
    // `as_millis` は u128 を返すが、経過ミリ秒が `u64::MAX`（約 5 億年）を超えることはないので飽和で足りる
    let elapsed_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
    if !self.quiet {
      write_terminal(&summary_line(&compilation.pdf_path, page_count, elapsed_ms, self.ansi));
    }
    if let Some(log) = &self.log {
      log.write_block(&summary_line(&compilation.pdf_path, page_count, elapsed_ms, false));
    }
  }

  /// ビルドを止めた致命的エラーの診断をログファイルへ記録する。
  ///
  /// 端末側は `termination::Outcome::report` が描くので、ここではファイルへだけ書く（`--quiet` でも書く）。
  /// `CompileFailure` の関連診断（`related`）も続けて描くので、`Failures` 集約の全 leaf が残る。
  pub(super) fn failure(&self, report: &miette::Report) {
    if let Some(log) = &self.log {
      log.write_block(&render_diagnostic_plain(report.as_ref()));
    }
  }

  /// `--log-file` の出力先（指定が無ければ `None`）。
  pub(super) fn log_path(&self) -> Option<&Path> { return self.log.as_ref().map(LogSink::path) }

  /// 実行記録の末尾（終了時刻・終了状態）を書いてからログの書き残しを流し切り、記録に失敗していれば
  /// それを返す。
  ///
  /// `succeeded` は本処理の成否。終了記録はほかのどの報告よりも後に書く。
  ///
  /// # Errors
  ///
  /// ログの書き込みまたは flush が失敗していたとき [`LogFailure`] を返す。
  pub(super) fn finish(self, succeeded: bool) -> Result<(), LogFailure> {
    let Some(log) = self.log else {
      return Ok(());
    };
    log.write_block(&run_footer_text(&now_text(&log.timer), succeeded));
    return log.finish();
  }
}

/// 端末（stderr）へ 1 件の報告を改行付きで書く。
///
/// 書けないとき（読み手が先に閉じたパイプ等）は報告を捨てる — 失敗を伝えられる出力先が無く、終了コードと
/// ログファイルへの記録は端末と独立に決まる（`termination::Outcome::report` と同じ扱い）。`text` は描画済みの
/// 文字列なので、書き込みが失敗しうるのは I/O だけ。
fn write_terminal(text: &str) { let _ = writeln!(io::stderr().lock(), "{text}"); }

/// ビルド成功サマリの 1 行を組み立てる。
fn summary_line(pdf_path: &Path, page_count: usize, elapsed_ms: u64, ansi: bool) -> String {
  let mark = if ansi {
    "\u{1b}[32m\u{2713}\u{1b}[0m"
  } else {
    "\u{2713}"
  };
  let path = pdf_path.display();
  return format!("{mark} {path} · {page_count} ページ · {elapsed_ms} ms");
}

/// 診断を描けなかったときの簡略表示の 1 行目。
const FALLBACK_HEADER: &str = "診断を描けなかったため、本文と code だけを示します（seiran の不具合です）";

/// 借用した診断を `handler` で描く表示用ラッパ。
///
/// `Report` の `Debug` は構築時に既定 handler（`set_hook` していなければ `MietteHandler::new()`。seiran は
/// `set_hook` を呼ばない）を捕まえて `handler.debug(診断, f)` を呼ぶだけなので、同じ handler で同じ関数を
/// 呼べば、出るバイト列は `Report` 経由のときと一致する。`Report` は所有した診断からしか作れないので、
/// `Warnings` が貸す `&dyn Diagnostic` を描くにはこの形が要る。
struct Rendered<'a>(&'a dyn ReportHandler, &'a dyn Diagnostic);

impl fmt::Debug for Rendered<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { return self.0.debug(self.1, f); }
}

/// ユーザー向け診断を `handler` で文字列へ描く。
///
/// 描画が失敗する（miette がラベルの span をソースから読めず `fmt::Error` を返す）のは、span を範囲内に保つ
/// 上流の不変条件が破れたときだけ。そのときは途中まで描いた分を捨て、[`FALLBACK_HEADER`] に続けて診断木
/// （自身 → `related` を前順）を 1 件 1 行の `  [code] 本文` で並べた簡略表示を返す。panic はせず、終了コードは
/// 呼び出し側の規則のまま変わらない。
pub(super) fn render_diagnostic(handler: &dyn ReportHandler, diagnostic: &dyn Diagnostic) -> String {
  let mut text = String::new();
  // `format!` は整形の失敗で panic するので、`fmt::Write` で `String` へ描いて結果を受け取る。
  if write!(text, "{:?}", Rendered(handler, diagnostic)).is_err() {
    return fallback_text(diagnostic);
  }
  return text;
}

/// [`render_diagnostic`] の簡略表示を組み立てる。
fn fallback_text(diagnostic: &dyn Diagnostic) -> String {
  let mut text = String::from(FALLBACK_HEADER);
  push_fallback_lines(&mut text, diagnostic);
  return text;
}

/// 簡略表示へ `diagnostic` とその関連診断を前順で 1 件 1 行ずつ足す。
fn push_fallback_lines(text: &mut String, diagnostic: &dyn Diagnostic) {
  // 書き込み先が `String` なので、失敗は診断自身の `Display` の失敗だけ。その行が欠けるだけで続けられる。
  let _ = match diagnostic.code() {
    Some(code) => write!(text, "\n  [{code}] {diagnostic}"),
    None => write!(text, "\n  {diagnostic}"),
  };
  for related in diagnostic.related().into_iter().flatten() {
    push_fallback_lines(text, related);
  }
}

/// ユーザー向け診断（warning・致命的エラー）をログファイル向けに装飾なしで文字列化する。
///
/// `related` を持つ診断は、端末と同じく関連診断まで続けて描く。描けないときの扱いは [`render_diagnostic`]。
fn render_diagnostic_plain(diagnostic: &dyn Diagnostic) -> String {
  // `new_themed` の既定はハイパーリンク有効で、url を持つ診断に OSC 8 のエスケープを出す。
  let handler = GraphicalReportHandler::new_themed(GraphicalTheme::unicode_nocolor()).with_links(false);
  return render_diagnostic(&handler, diagnostic);
}

/// stderr へ ANSI 装飾を出してよいか。
///
/// 装飾するのは `NO_COLOR` が未設定で、かつ出力先が端末のときだけ。`NO_COLOR` は「非空の値が
/// 設定されていれば装飾を止める」仕様に従い、値の中身は問わない。非 UTF-8 の値も設定とみなす点は
/// `tracing-subscriber` 既定の判定より厳しいが、仕様どおりなので合わせない。
fn ansi_enabled(no_color: Option<&OsStr>, stderr_is_terminal: bool) -> bool {
  return no_color.is_none_or(|value| return value.is_empty()) && stderr_is_terminal;
}

/// 現在時刻を `timer` の表現で文字列にする。
fn now_text(timer: &OffsetTime<Rfc3339>) -> String {
  let mut text = String::new();
  // 書き込み先が `String` なので書き込みは失敗しない。整形の失敗は表現できない日付（`time` の範囲外）だけで、
  // 現在時刻では起きない — 起きても時刻が空になるだけで記録そのものは続けられる。
  let _ = timer.format_time(&mut Writer::new(&mut text));
  return text;
}

/// ログファイルの先頭に書く実行記録のうち、呼び出し側が決める部分。
pub(super) struct RunHeader<'a> {
  /// 入力パスを含む実効サブコマンド
  pub(super) subcommand: &'a Command,
  /// 相対パスの解決基準（起動時のカレントディレクトリ。取得できなかったときは `None`）
  pub(super) base_dir: Option<&'a Path>,
}

/// 実行記録の先頭ブロックを組み立てる。
fn run_header_text(started_at: &str, header: &RunHeader<'_>, directive: &str) -> String {
  let base_dir = match header.base_dir {
    Some(dir) => dir.display().to_string(),
    None => String::from("（取得できませんでした）"),
  };
  return format!(
    "# seiran 実行記録\n開始時刻: {started_at}\nバージョン: {}\nサブコマンド: {}\n基準ディレクトリ: \
     {base_dir}\n実効フィルタ: {directive}",
    env!("CARGO_PKG_VERSION"),
    header.subcommand,
  );
}

/// 実行記録の末尾ブロックを組み立てる。
///
/// 終了状態は本処理の成否。ログの記録そのものの失敗（終了コード 1 になる）は、記録できない出力先へ
/// 書けないので含まない。
fn run_footer_text(ended_at: &str, succeeded: bool) -> String {
  let status = if succeeded { "成功" } else { "失敗" };
  return format!("# seiran 実行終了\n終了時刻: {ended_at}\n終了状態: {status}");
}

#[cfg(test)]
mod tests {
  use std::{ffi::OsStr, path::Path};

  use miette::{Diagnostic, MietteError, MietteHandler, MietteHandlerOpts, SourceCode, SourceSpan, SpanContents};
  use thiserror::Error;
  use time::{UtcOffset, format_description::well_known::Rfc3339};
  use tracing_subscriber::fmt::time::OffsetTime;

  use super::{
    FALLBACK_HEADER, RunHeader, ansi_enabled, now_text, render_diagnostic, render_diagnostic_plain, run_header_text,
    summary_line,
  };
  use crate::cli::Command;

  /// 体裁の確認に使う warning 診断。
  #[derive(Debug, Error, Diagnostic)]
  #[error("テスト用の警告です")]
  #[diagnostic(
    code(cli::test_warning),
    severity(Warning),
    help("ヘルプも装飾なしで出る"),
    url("https://example.com/warning")
  )]
  struct TestWarning;

  /// ソースの範囲外を指すラベルを持つ診断。
  #[derive(Debug, Error, Diagnostic)]
  #[error("範囲外のラベルを持つ診断です")]
  #[diagnostic(code(cli::test_out_of_range))]
  struct OutOfRangeLabel {
    /// ラベルが指す先のソース
    #[source_code]
    text: String,
    /// ソースの末尾より後ろを指すラベル
    #[label("ここ")]
    span: SourceSpan,
  }

  /// 範囲外の関連診断を持つ、それ自身は描ける診断。
  #[derive(Debug, Error, Diagnostic)]
  #[error("関連を持つ診断です")]
  #[diagnostic(code(cli::test_with_related))]
  struct WithRelated {
    /// 描けない関連診断
    #[related]
    notes: Vec<OutOfRangeLabel>,
  }

  /// 前後の文脈付きの読み出しにだけ応じるソース。
  ///
  /// graphical handler は文脈付きで読めたラベルを、行と桁を出すために文脈なしで読み直す。そこで拒むと、
  /// 見出しを書いた後で `fmt::Error` を返す。
  #[derive(Debug)]
  struct ContextOnlySource(String);

  impl SourceCode for ContextOnlySource {
    fn read_span<'a>(
      &'a self,
      span: &SourceSpan,
      context_lines_before: usize,
      context_lines_after: usize,
    ) -> Result<Box<dyn SpanContents<'a> + 'a>, MietteError> {
      if context_lines_before == 0 {
        return Err(MietteError::OutOfBounds);
      }
      return self.0.read_span(span, context_lines_before, context_lines_after);
    }
  }

  /// graphical handler が描画の途中で失敗する診断。
  #[derive(Debug, Error, Diagnostic)]
  #[error("読み直せないソースを持つ診断です")]
  #[diagnostic(code(cli::test_context_only))]
  struct ContextOnlyLabel {
    /// 文脈なしの読み出しを拒むソース
    #[source_code]
    text: ContextOnlySource,
    /// ソースの範囲内を指すラベル
    #[label("ここ")]
    span: SourceSpan,
  }

  /// 3 文字のソースの 10 文字目から 2 文字を指す診断を作る。
  fn out_of_range() -> OutOfRangeLabel {
    return OutOfRangeLabel {
      text: String::from("abc"),
      span: SourceSpan::from((10, 2)),
    };
  }

  /// 文章による描画（`NO_GRAPHICS` を設定した端末で既定 handler が選ぶもの）。
  fn narrated() -> MietteHandler { return MietteHandlerOpts::new().force_narrated(true).build(); }

  #[test]
  fn out_of_range_label_falls_back_with_the_narrated_handler() {
    let text = render_diagnostic(&narrated(), &out_of_range());

    assert!(text.starts_with(FALLBACK_HEADER), "途中まで描いた分を混ぜない: {text}");
    assert!(text.contains("  [cli::test_out_of_range] 範囲外のラベルを持つ診断です"), "{text}");
  }

  #[test]
  fn out_of_range_label_keeps_code_and_message_with_the_graphical_handler() {
    let text = render_diagnostic(&MietteHandlerOpts::new().force_graphical(true).build(), &out_of_range());

    assert!(text.contains("cli::test_out_of_range"), "{text}");
    assert!(text.contains("範囲外のラベルを持つ診断です"), "{text}");
  }

  #[test]
  fn fallback_lists_related_diagnostics_too() {
    let diagnostic = WithRelated {
      notes: vec![out_of_range()],
    };

    let text = render_diagnostic(&narrated(), &diagnostic);

    assert_eq!(
      text,
      format!(
        "{FALLBACK_HEADER}\n  [cli::test_with_related] 関連を持つ診断です\n  [cli::test_out_of_range] \
         範囲外のラベルを持つ診断です"
      )
    );
  }

  #[test]
  fn plain_rendering_discards_partial_output_on_failure() {
    let diagnostic = ContextOnlyLabel {
      text: ContextOnlySource(String::from("abc")),
      span: SourceSpan::from((0, 1)),
    };

    let text = render_diagnostic_plain(&diagnostic);

    assert_eq!(text, format!("{FALLBACK_HEADER}\n  [cli::test_context_only] 読み直せないソースを持つ診断です"));
  }

  #[test]
  fn ansi_needs_both_terminal_and_unset_no_color() {
    assert!(ansi_enabled(None, true), "NO_COLOR 未設定かつ端末なら装飾する");
    assert!(!ansi_enabled(None, false), "端末でなければ装飾しない");
    assert!(!ansi_enabled(Some(OsStr::new("1")), true), "NO_COLOR が非空なら端末でも装飾しない");
    assert!(ansi_enabled(Some(OsStr::new("")), true), "NO_COLOR が空文字なら未設定として扱う");
  }

  #[test]
  fn summary_line_is_undecorated_without_ansi() {
    let line = summary_line(Path::new("out/main.pdf"), 12, 843, false);

    assert_eq!(line, "\u{2713} out/main.pdf · 12 ページ · 843 ms");
    assert!(!line.contains('\u{1b}'), "装飾なしでは ESC を含まない");
  }

  #[test]
  fn summary_line_is_decorated_with_ansi() {
    let line = summary_line(Path::new("out/main.pdf"), 12, 843, true);

    assert!(line.contains('\u{1b}'), "装飾ありでは完了記号を着色する");
    assert!(line.ends_with("out/main.pdf · 12 ページ · 843 ms"));
  }

  #[test]
  fn rendered_warning_has_no_ansi() {
    let rendered = render_diagnostic_plain(&TestWarning);

    assert!(!rendered.contains('\u{1b}'), "url を持つ診断でもハイパーリンクの ESC を入れない");
    assert!(rendered.contains("テスト用の警告です"), "本文はそのまま残す");
  }

  #[test]
  fn terminal_rendering_of_a_borrowed_diagnostic_matches_the_report() {
    let expected = format!("{:?}", miette::Report::new(TestWarning));

    let rendered = render_diagnostic(&MietteHandler::new(), &TestWarning);

    assert_eq!(rendered, expected);
  }

  #[test]
  fn run_header_marks_an_unknown_base_dir() {
    let header = RunHeader {
      subcommand: &Command::Build {
        config_path: "config.toml".into(),
      },
      base_dir: None,
    };

    let text = run_header_text("t", &header, "warn");

    assert!(text.contains("基準ディレクトリ: （取得できませんでした）"), "{text}");
  }

  #[test]
  fn utc_offset_timer_writes_z_suffix() {
    // オフセット 0 の `OffsetTime` は `UtcTime::rfc_3339()` と同じく `Z` で終わる（ローカル時刻を取れないときの退避先）
    let text = now_text(&OffsetTime::new(UtcOffset::UTC, Rfc3339));

    assert!(text.ends_with('Z'), "UTC は Z 付きの RFC 3339 で書く: {text}");
  }
}
