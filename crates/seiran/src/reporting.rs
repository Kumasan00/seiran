//! CLI のユーザー向け報告と開発者向け tracing 設定
//!
//! 出力を、warning 診断と成功サマリからなるユーザー向け報告と、処理観測用の tracing に分ける。
//! 呼び出し側は [`Reporter`] の初期化と報告操作だけを知り、フィルタ優先順位・表示形式・端末装飾は
//! 本 module に閉じる。端末側の出力先は stderr で、stdout はパイプできる成果物のための経路として空けておく。
//!
//! `--log-file` を指定したときは、端末の出力をそのままに**ログファイルを足す**。ファイルには先頭の実行記録・
//! tracing イベント・warning 診断・成功サマリ・致命的エラー診断・末尾の終了記録を、装飾なし・イベントには
//! 時刻付きで残す。実行記録と終了記録は tracing を通さないので、フィルタに依らず残る。

mod log_file;

use std::{ffi::OsStr, io::IsTerminal, path::Path, sync::Arc, time::Duration};

use log_file::LogSink;
pub(super) use log_file::{LogFailure, LogFileError};
use miette::{Diagnostic, GraphicalReportHandler, GraphicalTheme, MietteHandler, ReportHandler};
use thiserror::Error;
use tracing_subscriber::{
  EnvFilter, Registry,
  filter::{LevelFilter, ParseError},
  fmt::{
    self,
    format::Writer,
    time::{FormatTime, OffsetTime, UtcTime},
  },
  layer::{Layer, SubscriberExt},
  util::SubscriberInitExt,
};

/// 端末への出力を止めるフィルタ directive。
const QUIET_DIRECTIVE: &str = "off";

/// 実効フィルタの決定に伴う、ユーザーが直せる通知。
///
/// tracing の WARN では出さない — 通知の対象である `RUST_LOG` 自身が WARN を通さない指定（`error` / target
/// 限定）だと、通知が消えてしまうため。warning 診断として [`Reporter::warning`] が端末（`-q` 以外）と
/// ログファイルへ出す（#551）。
#[derive(Debug, Error, Diagnostic)]
enum FilterWarning {
  /// `RUST_LOG` を解釈できず、`--verbose` の設定へ戻した
  #[error("環境変数 RUST_LOG を解釈できないため、--verbose の設定を使用します")]
  #[diagnostic(
    code(cli::rust_log::invalid),
    severity(Warning),
    help(
      "RUST_LOG の書式（例: seiran_compiler::typeset=trace）を確認するか、RUST_LOG を外して -v / -vv / -vvv を使ってください。"
    )
  )]
  Invalid {
    /// 解釈の失敗
    #[source]
    source: ParseError,
  },

  /// 有効な `RUST_LOG` が `--verbose` を覆った
  #[error("環境変数 RUST_LOG が設定されているため、--verbose の指定を無視します: RUST_LOG={value}")]
  #[diagnostic(
    code(cli::rust_log::overrides_verbose),
    severity(Warning),
    help("-v を効かせるには RUST_LOG を外してください。RUST_LOG で詳細度を決めるなら -v は不要です。")
  )]
  OverridesVerbose {
    /// 設定されていた `RUST_LOG` の値
    value: String,
  },
}

/// CLI のユーザー向け報告器。
///
/// `quiet` の解釈と ANSI 装飾の可否を保持し、warning と成功サマリへ一貫して適用する。tracing subscriber は
/// [`Reporter::init`] でプロセス全体に 1 回だけ初期化する。
///
/// `--log-file` 指定時はログファイルの書き出し口も保持する。書き出しは同期で、書き込み・flush の失敗は
/// sink が保持し、[`Reporter::finish`] が 1 度だけ取り出す — `main` は本処理の結果とログの結果の両方を
/// 受けてから終了コードを決める。
pub(super) struct Reporter {
  /// 端末への非エラー出力を抑止するか。
  quiet: bool,
  /// stderr へ ANSI 装飾を出してよいか。
  ansi: bool,
  /// ログファイルへの書き出し口（`--log-file` 指定時のみ）。
  log: Option<LogSink>,
  /// 端末へ warning 診断を描く miette の既定 handler。`Report` の `Debug` 表示が使うのと同じもので、
  /// 端末幅・色・unicode の判定を [`Reporter::init`] で 1 回だけ行うためにここへ持つ。
  terminal: MietteHandler,
  /// ログファイルの時刻表現（実行記録の開始・終了時刻に使う。イベントの時刻と同じ）
  timer: LogTimer,
}

impl Reporter {
  /// tracing を初期化し、同じ quiet 方針と装飾方針を持つ報告器を返す。
  ///
  /// フィルタの優先順位は `RUST_LOG`、`--verbose`、既定値の順で、`--verbose` が詳細化するのは Seiran 自身の
  /// 3 target だけ（依存 crate は WARN のまま）。有効な `RUST_LOG` が `--verbose` を覆うとき・`RUST_LOG` を
  /// 解釈できないときは warning 診断を 1 件出す（実効フィルタを通らない）。
  /// `--quiet` は端末側のフィルタを `off` にするだけで、ログファイルの内容は減らさない — 「静かに回して
  /// 後で読む」がファイル出力の目的だから。`--verbose` とは独立で、`-q -vv --log-file` は端末を黙らせたまま
  /// ファイルだけ詳しくする。
  ///
  /// 端末側の出力先は stderr を明示する（`fmt` の既定は stdout で、そのままではログが成果物の経路へ流れる）。
  /// 端末装飾の可否はここで 1 回だけ決め、ログ（`with_ansi`）と成功サマリで同じ値を使う。`fmt` の既定は
  /// `NO_COLOR` しか見ず出力先が端末かを問わないため、明示的に与える必要がある（#493）。
  ///
  /// `--log-file` 指定時は、subscriber を設置する前にファイルの先頭へ実行記録（開始時刻・バージョン・
  /// サブコマンド・基準ディレクトリ・実効フィルタ）を書く。tracing を通さないのでフィルタに依らず残る。
  ///
  /// # Errors
  ///
  /// `--log-file` のパスを開けないとき [`LogFileError`] を返す。ログが残らないまま処理が進むより、
  /// 指定が効いていないことを即座に知らせる。
  pub(super) fn init(
    verbose: u8,
    quiet: bool,
    log_file: Option<&Path>,
    header: &RunHeader<'_>,
  ) -> Result<Self, LogFileError> {
    // ローカル時刻の解決を先に済ませる（`OffsetTime::local_rfc_3339` はプロセスが単一スレッドのうちに解決する）。
    let timer = log_timer();
    let raw_filter = std::env::var("RUST_LOG").ok();
    let plan = build_log_plan(raw_filter.as_deref(), verbose, quiet, log_file.is_some());
    let ansi = ansi_enabled(std::env::var_os("NO_COLOR").as_deref(), std::io::stderr().is_terminal());
    let log = log_file.map(LogSink::open).transpose()?;
    // 実行記録の先頭は subscriber を設置する前に書く — 以後のどの event・通知よりも前に来ることを、
    // 書く順序そのもので保証する。
    if let Some(log) = &log {
      log.write_block(&run_header_text(&now_text(&timer), header, &plan.directive));
    }

    let stderr_layer = fmt::layer()
      .compact()
      .with_target(plan.stderr.show_target)
      .with_writer(std::io::stderr)
      .with_ansi(ansi)
      .with_file(false)
      .with_line_number(false)
      .without_time()
      .with_filter(plan.stderr.filter);
    let file_layer = log.as_ref().zip(plan.file).map(|(sink, sink_plan)| {
      return fmt::layer()
        .compact()
        .with_target(sink_plan.show_target)
        .with_writer(sink.writer())
        .with_ansi(false)
        .with_file(false)
        .with_line_number(false)
        .with_timer(timer.clone())
        // 書き込み失敗は sink が保持して `finish` が報告するので、layer 側から stderr へ出させない
        // （出すと同じ失敗が 2 回出るうえ、`--log-file` の有無で stderr のバイト列が変わる）。
        // このフラグは書き込み失敗だけでなく event の整形失敗の報告も同じく抑止する。整形失敗は
        // `compact()` と seiran が出すフィールドの単純さからいって理論上のものでしかなく、しかも
        // 何も書かれないので sink 側も保持しようがない（保持できるのは writer へ渡った後の失敗だけ）。
        .log_internal_errors(false)
        .with_filter(sink_plan.filter);
    });
    Registry::default().with(stderr_layer).with(file_layer).init();

    let reporter = Reporter {
      quiet,
      ansi,
      log,
      terminal: MietteHandler::new(),
      timer,
    };
    if let Some(warning) = &plan.warning {
      reporter.warning(warning);
    }
    return Ok(reporter);
  }

  /// コンパイルが返した warning 診断を報告する。
  ///
  /// 端末へは miette の既定 handler で描き、致命的エラー（`Report` の `Debug` 表示）と同じ体裁にする
  /// （[`TerminalDiagnostic`]）。tracing へは複製しないため、同じ問題が 1 つの出力先へ 2 回出ることはない。
  /// ログファイルへは装飾なしで書き、`--quiet` でも省かない — warning の抜けた記録は事後解析に使えないため。
  pub(super) fn warnings(&self, warnings: &seiran_compiler::Warnings) {
    for warning in warnings {
      self.warning(warning);
    }
  }

  /// warning 診断 1 件を報告する。
  ///
  /// 端末へは `--quiet` でなければ miette の既定 handler で描き、ログファイルへは装飾なしで常に書く。
  /// compile の警告と CLI 自身の通知（[`FilterWarning`]）が同じ体裁・同じ振り分けで出る。
  fn warning(&self, diagnostic: &dyn Diagnostic) {
    if !self.quiet {
      eprintln!("{:?}", TerminalDiagnostic(&self.terminal, diagnostic));
    }
    if let Some(log) = &self.log {
      log.write_block(&render_diagnostic_plain(diagnostic));
    }
  }

  /// ビルド成功時のサマリを報告する。
  ///
  /// 時間は compiler だけでなく render と保存を含む CLI の build 全体。完了記号を着色するかは
  /// [`Reporter::init`] が決めた 1 つの判定に従うため、ログの装飾と食い違わない。ログファイルへは
  /// 装飾なしで書き、`--quiet` でも省かない。
  pub(super) fn build(&self, compilation: &seiran_compiler::Compilation, elapsed: Duration) {
    let page_count = compilation.statistics.page_count;
    // `as_millis` は u128 を返すが、経過ミリ秒が `u64::MAX`（約 5 億年）を超えることはないので飽和で足りる
    let elapsed_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
    if !self.quiet {
      eprintln!("{}", summary_line(&compilation.pdf_path, page_count, elapsed_ms, self.ansi));
    }
    if let Some(log) = &self.log {
      log.write_block(&summary_line(&compilation.pdf_path, page_count, elapsed_ms, false));
    }
  }

  /// ビルドを止めた致命的エラーの診断をログファイルへ記録する。
  ///
  /// 端末側は `termination::Outcome::report` が miette のグローバル handler（`Report` の `Debug` 表示）で
  /// 1 回だけ描くのでここでは触らない — 端末とファイルで同じ診断が 1 回ずつ。`--quiet` は端末だけを黙らせるものなので、ファイルへは
  /// 常に書く（`-q --log-file` で失敗理由がどこにも残らない経路を無くすのがこの操作の目的）。体裁は warning と
  /// 同じ装飾なし・ハイパーリンクなし・時刻なしで、`CompileFailure` の関連診断（`related`）も同じ handler が
  /// 続けて描くため、`Failures` 集約の全 leaf が残る。tracing の ERROR event には流さない — 致命的エラーは
  /// miette で報告し ERROR レベルは使わないという線引き（#103）を、ファイルでも保つ。
  pub(super) fn failure(&self, report: &miette::Report) {
    if let Some(log) = &self.log {
      log.write_block(&render_diagnostic_plain(report.as_ref()));
    }
  }

  /// `--log-file` の出力先（指定が無ければ `None`）。
  ///
  /// PDF の保存先と同じ実体を指していないかを保存前に確かめるために公開する。
  pub(super) fn log_path(&self) -> Option<&Path> { return self.log.as_ref().map(LogSink::path) }

  /// 実行記録の末尾（終了時刻・終了状態）を書いてからログの書き残しを流し切り、記録に失敗していれば
  /// それを返す。
  ///
  /// `succeeded` は本処理の成否。終了記録はほかのどの報告よりも後に書く（`main` は致命的エラーの記録を
  /// 済ませてからこれを呼ぶ）。
  ///
  /// # Errors
  ///
  /// ログの書き込みまたは flush が失敗していたとき [`LogFailure`] を返す。
  pub(super) fn finish(self, succeeded: bool) -> Result<(), LogFailure> {
    let Some(log) = self.log else {
      return Ok(());
    };
    log.write_block(&run_footer_text(&now_text(&self.timer), succeeded));
    return log.finish();
  }
}

/// ビルド成功サマリの 1 行を組み立てる。
///
/// 装飾の有無だけが出力先ごとに変わるので、体裁はここ 1 箇所に置いて端末とファイルで共有する。
fn summary_line(pdf_path: &Path, page_count: usize, elapsed_ms: u64, ansi: bool) -> String {
  let mark = if ansi {
    "\u{1b}[32m\u{2713}\u{1b}[0m"
  } else {
    "\u{2713}"
  };
  let path = pdf_path.display();
  return format!("{mark} {path} · {page_count} ページ · {elapsed_ms} ms");
}

/// 借用した診断を、miette の既定 handler で端末向けに描く表示用ラッパ。
///
/// `Report` は所有した診断からしか作れないので、`Warnings` が貸す `&dyn Diagnostic` を端末へ出すにはこの
/// 形が要る。`Report` の `Debug` は構築時に既定 handler（`set_hook` していなければ `MietteHandler::new()`。
/// seiran は `set_hook` を呼ばない）を捕まえて `handler.debug(診断, f)` を呼ぶだけなので、同じ handler で
/// 同じ関数を呼べば、端末へ出るバイト列は `Report` 経由のときと一致する（#550）。
struct TerminalDiagnostic<'a>(&'a MietteHandler, &'a dyn Diagnostic);

impl std::fmt::Debug for TerminalDiagnostic<'_> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { return self.0.debug(self.1, f); }
}

/// ユーザー向け診断（warning・致命的エラー）をログファイル向けに装飾なしで文字列化する。
///
/// 端末側は既定の miette handler に任せたままにする — ここで体裁を作るのは「出力先が tty でないファイル」の
/// ためだけで、端末の見え方は `--log-file` の有無で変わらない。`related` を持つ診断は、端末と同じく
/// 関連診断まで続けて描く。
fn render_diagnostic_plain(diagnostic: &dyn Diagnostic) -> String {
  let mut rendered = String::new();
  // `new_themed` の既定はハイパーリンク有効で、url を持つ診断に OSC 8 のエスケープを出す。
  let handler = GraphicalReportHandler::new_themed(GraphicalTheme::unicode_nocolor()).with_links(false);
  // 書き込み先が `String` なので失敗しない。
  let _ = handler.render_report(&mut rendered, diagnostic);
  return rendered;
}

/// stderr へ ANSI 装飾を出してよいか。
///
/// 装飾するのは `NO_COLOR` が未設定で、かつ出力先が端末のときだけ。`NO_COLOR` は「非空の値が
/// 設定されていれば装飾を止める」仕様に従い、値の中身は問わない。非 UTF-8 の値も設定とみなす点は
/// `tracing-subscriber` 既定の判定より厳しいが、仕様どおりなので合わせない。
fn ansi_enabled(no_color: Option<&OsStr>, stderr_is_terminal: bool) -> bool {
  return no_color.is_none_or(|value| return value.is_empty()) && stderr_is_terminal;
}

/// ログファイルへ書く時刻の表現。
///
/// ローカル時刻とその UTC フォールバックで型が違うので 1 つの型へ畳む。tracing の layer（イベントの時刻）と
/// 実行記録（開始・終了時刻）が同じ表現を使うよう、`Arc` で共有する。
#[derive(Clone)]
struct LogTimer(Arc<dyn FormatTime + Send + Sync>);

impl FormatTime for LogTimer {
  fn format_time(&self, writer: &mut Writer<'_>) -> std::fmt::Result { return self.0.format_time(writer); }
}

/// ログファイル用の時刻表現を決める。
///
/// 事後に読むログは手元の時計と突き合わせられたほうがよいのでローカル時刻を採る。オフセットを取得できない
/// 環境では UTC へ落とす — 時刻が無いログよりは、ずれの分かる時刻があるほうが使える。
fn log_timer() -> LogTimer {
  return match OffsetTime::local_rfc_3339() {
    Ok(timer) => LogTimer(Arc::new(timer)),
    // オフセットの取得失敗そのものは報告しない（ログの体裁の話で、ビルドの成否には関わらない）。
    Err(_) => LogTimer(Arc::new(UtcTime::rfc_3339())),
  };
}

/// 現在時刻を `timer` の表現で文字列にする。
fn now_text(timer: &LogTimer) -> String {
  let mut text = String::new();
  // 書き込み先が `String` なので書き込みは失敗しない。整形の失敗は表現できない日付（`time` の範囲外）だけで、
  // 現在時刻では起きない — 起きても時刻が空になるだけで記録そのものは続けられる。
  let _ = timer.format_time(&mut Writer::new(&mut text));
  return text;
}

/// ログファイルの先頭に書く実行記録のうち、呼び出し側が決める部分。
pub(super) struct RunHeader<'a> {
  /// 実行したサブコマンド（コマンドラインの綴り）
  pub(super) subcommand: &'static str,
  /// 相対パスの解決基準（起動時のカレントディレクトリ。取得できなかったときは `None`）
  pub(super) base_dir: Option<&'a Path>,
}

/// 実行記録の先頭ブロックを組み立てる。
///
/// tracing を通さずファイルへ直接書くので、`-v` も `RUST_LOG` も無い実行でも「何の記録か」が分かる。
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

/// 両方の出力先に共通する実効フィルタの directive と、その決定に伴う通知。
struct FilterChoice {
  /// 実効フィルタの directive。
  directive: String,
  /// subscriber 初期化後に報告する通知。
  warning: Option<FilterWarning>,
}

/// 出力先 1 つぶんのフィルタと表示設定。
struct SinkPlan {
  /// 実効フィルタ。
  filter: EnvFilter,
  /// イベントの target（module パス）を表示するか。
  show_target: bool,
}

impl SinkPlan {
  /// フィルタから表示設定を導いた計画を作る。
  fn new(filter: EnvFilter) -> Self {
    let show_target = shows_target(&filter);
    return SinkPlan {
      filter,
      show_target,
    };
  }
}

/// 出力先ごとの計画。
struct LogPlan {
  /// 端末（stderr）側の計画。
  stderr: SinkPlan,
  /// ログファイル側の計画（`--log-file` 指定時のみ）。
  file: Option<SinkPlan>,
  /// 両方の出力先に共通する実効フィルタの directive（実行記録に書く。`--quiet` の端末側 `off` は含まない）
  directive: String,
  /// subscriber 初期化後に報告する通知。
  warning: Option<FilterWarning>,
}

/// 優先順位に従って出力先ごとのフィルタを構築する。
///
/// `--quiet` は「端末をうるさくするな」の意味に限定し、フィルタの決定ではなく端末側への適用だけに効かせる。
/// ログファイル側は `--quiet` を見ないので、`-q --log-file x.log` は端末に何も出さずファイルへは通常どおり書く。
/// `--verbose` とは直交で、`-q -vv --log-file x.log` は端末無言のままファイルへ DEBUG まで書く。`--log-file` の
/// 無い `-q -vv` は矛盾ではなく効果が無いだけなので、警告もエラーも出さない（`-v` を常に付ける運用に `-q` を
/// 足せる）。`EnvFilter` は `Clone` できないため、共通の directive を 1 度決めて出力先ごとに parse し直す。
fn build_log_plan(raw_filter: Option<&str>, verbose: u8, quiet: bool, has_log_file: bool) -> LogPlan {
  let choice = resolve_filter(raw_filter, verbose);
  let stderr_directive = if quiet {
    QUIET_DIRECTIVE
  } else {
    choice.directive.as_str()
  };
  let stderr = SinkPlan::new(parse_directive(stderr_directive));
  let file = has_log_file.then(|| return SinkPlan::new(parse_directive(&choice.directive)));
  return LogPlan {
    stderr,
    file,
    directive: choice.directive,
    warning: choice.warning,
  };
}

/// 妥当性を確認済みの directive から `EnvFilter` を作る。
///
/// `EnvFilter::new` は使わない — 不正な directive を stderr へ通知して黙って捨てるうえ、大域の既定
/// directive を足すため、`RUST_LOG` に書いたとおりの実効フィルタにならない。ここへ渡る directive は
/// [`resolve_filter`] が strict な parse で通したものか静的な既定値なので、取りこぼしは起きない。
fn parse_directive(directive: &str) -> EnvFilter { return EnvFilter::builder().parse_lossy(directive); }

/// 両方の出力先が使う directive を決める。
///
/// `RUST_LOG` が有効ならそれが全権で、`--verbose` は無視する — 合成（フラグの directive へ `RUST_LOG` を
/// 上書きで足す等）はしない。同一 target への複数 directive の優先規則に依存して、実効フィルタが字面から
/// 読めなくなるため。代わりに `--verbose` が 1 段以上あれば「無視した」と警告する（`--verbose` 未指定なら
/// 警告しない — `RUST_LOG` だけで制御する開発者運用を汚さない）。`RUST_LOG` が不正なら CLI の verbose 設定へ
/// 戻し、こちらも警告する。
///
/// 通知はどちらも [`FilterWarning`] の warning 診断で、実効フィルタを通らない — `RUST_LOG` が WARN を
/// 通さない指定でも、端末（`-q` 以外）とログファイルに出る。優先順位そのものは変えない（#551）。
fn resolve_filter(raw_filter: Option<&str>, verbose: u8) -> FilterChoice {
  if let Some(raw) = raw_filter
    && !raw.trim().is_empty()
  {
    match EnvFilter::builder().parse(raw) {
      Ok(_) => {
        let warning = (verbose > 0).then(|| {
          return FilterWarning::OverridesVerbose {
            value: raw.to_owned(),
          };
        });
        return FilterChoice {
          directive: raw.to_owned(),
          warning,
        };
      },
      Err(source) => {
        return FilterChoice {
          directive: flag_directive(verbose).to_owned(),
          warning: Some(FilterWarning::Invalid { source }),
        };
      },
    }
  }
  return FilterChoice {
    directive: flag_directive(verbose).to_owned(),
    warning: None,
  };
}

/// フィルタが TRACE を出しうるか。
///
/// TRACE は文書の中身に比例して出るため、どの module 由来かが分からないと読めない。そこで target 表示は
/// `--verbose` の段数ではなく実効フィルタの上限で決める — `RUST_LOG` で TRACE を要求したときも表示される。
fn shows_target(filter: &EnvFilter) -> bool {
  return <EnvFilter as Layer<Registry>>::max_level_hint(filter).is_none_or(|hint| return hint >= LevelFilter::TRACE);
}

/// `--verbose` に対応するフィルタ directive を返す。
fn flag_directive(verbose: u8) -> &'static str {
  return match verbose {
    0 => "warn",
    1 => "warn,seiran=info,seiran_compiler=info,seiran_pdf=info",
    2 => "warn,seiran=debug,seiran_compiler=debug,seiran_pdf=debug",
    _ => "warn,seiran=trace,seiran_compiler=trace,seiran_pdf=trace",
  };
}

#[cfg(test)]
mod tests {
  use std::{ffi::OsStr, path::Path};

  use miette::{Diagnostic, MietteHandler};
  use thiserror::Error;

  use super::{
    FilterWarning, RunHeader, TerminalDiagnostic, ansi_enabled, build_log_plan, flag_directive, parse_directive,
    render_diagnostic_plain, run_footer_text, run_header_text, summary_line,
  };

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

  /// `Failures` 集約の leaf を模した error 診断。
  #[derive(Debug, Error, Diagnostic)]
  #[error("関連診断を持つテストエラーです")]
  #[diagnostic(code(cli::test_primary), help("主診断のヘルプ"))]
  struct TestPrimary {
    /// 主診断に続けて描かれる残りの leaf
    #[related]
    rest: Vec<TestRelated>,
  }

  /// 主診断の後ろに並ぶ関連診断。
  #[derive(Debug, Error, Diagnostic)]
  #[error("関連するテストエラーです")]
  #[diagnostic(code(cli::test_related), help("関連診断のヘルプ"))]
  struct TestRelated;

  /// `--verbose` の段数に対応する directive をフィルタ表記へ揃える。
  fn flag_filter_text(verbose: u8) -> String { return parse_directive(flag_directive(verbose)).to_string(); }

  #[test]
  fn verbose_only_increases_seiran_targets() {
    assert_eq!(flag_directive(0), "warn");
    assert_eq!(flag_directive(1), "warn,seiran=info,seiran_compiler=info,seiran_pdf=info");
    assert_eq!(flag_directive(2), "warn,seiran=debug,seiran_compiler=debug,seiran_pdf=debug");
    assert_eq!(flag_directive(3), "warn,seiran=trace,seiran_compiler=trace,seiran_pdf=trace");
  }

  #[test]
  fn quiet_silences_only_the_terminal() {
    let plan = build_log_plan(Some("trace"), 3, true, false);

    assert_eq!(plan.stderr.filter.to_string(), "off");
    assert!(plan.file.is_none(), "--log-file がなければファイル側の計画は作らない");
    assert!(!plan.stderr.show_target, "抑止時は target を表示しない");
  }

  #[test]
  fn quiet_keeps_the_log_file_verbose() {
    let plan = build_log_plan(None, 3, true, true);
    let file = plan.file.expect("--log-file 指定時はファイル側の計画がある");

    assert_eq!(plan.stderr.filter.to_string(), "off", "端末は黙る");
    assert_eq!(file.filter.to_string(), flag_filter_text(3), "ファイルは -q を見ない");
    assert!(!plan.stderr.show_target);
    assert!(file.show_target, "ファイル側は TRACE を出すので target を表示する");
  }

  #[test]
  fn both_sinks_share_the_same_filter() {
    let plan = build_log_plan(Some("seiran_compiler=trace"), 0, false, true);
    let file = plan.file.expect("--log-file 指定時はファイル側の計画がある");

    assert_eq!(plan.stderr.filter.to_string(), "seiran_compiler=trace");
    assert_eq!(file.filter.to_string(), plan.stderr.filter.to_string());
  }

  #[test]
  fn valid_rust_log_takes_priority_over_verbose() {
    let plan = build_log_plan(Some("seiran_compiler=trace"), 0, false, false);

    assert_eq!(plan.stderr.filter.to_string(), "seiran_compiler=trace");
    assert!(plan.warning.is_none());
  }

  #[test]
  fn invalid_rust_log_falls_back_to_verbose() {
    let plan = build_log_plan(Some("seiran=not-a-level"), 1, false, false);

    assert_eq!(plan.stderr.filter.to_string(), flag_filter_text(1));
    assert!(matches!(plan.warning, Some(FilterWarning::Invalid { .. })));
  }

  #[test]
  fn invalid_rust_log_warning_survives_quiet() {
    let plan = build_log_plan(Some("seiran=not-a-level"), 1, true, true);
    let file = plan.file.expect("--log-file 指定時はファイル側の計画がある");

    assert_eq!(file.filter.to_string(), flag_filter_text(1));
    assert!(plan.warning.is_some(), "端末が黙っていても警告文はログファイルへ残す");
  }

  #[test]
  fn valid_rust_log_with_verbose_warns_override() {
    let plan = build_log_plan(Some("info"), 3, false, false);

    assert_eq!(plan.stderr.filter.to_string(), "info", "実効フィルタは RUST_LOG のまま");
    assert!(
      matches!(&plan.warning, Some(FilterWarning::OverridesVerbose { value }) if value == "info"),
      "覆った RUST_LOG の値を持つ"
    );
  }

  #[test]
  fn blank_rust_log_with_verbose_does_not_warn() {
    for raw in [None, Some(""), Some("  ")] {
      let plan = build_log_plan(raw, 2, false, false);

      assert_eq!(plan.stderr.filter.to_string(), flag_filter_text(2), "{raw:?} は未設定として --verbose が効く");
      assert!(plan.warning.is_none(), "{raw:?} では警告しない");
    }
  }

  #[test]
  fn invalid_rust_log_with_verbose_warns_only_about_parse() {
    let plan = build_log_plan(Some("seiran=not-a-level"), 2, false, false);

    assert!(
      matches!(plan.warning, Some(FilterWarning::Invalid { .. })),
      "不正値の通知だけを出し、無視の通知は重ねない"
    );
  }

  #[test]
  fn quiet_and_verbose_write_debug_only_to_the_file() {
    let plan = build_log_plan(None, 2, true, true);
    let file = plan.file.expect("--log-file 指定時はファイル側の計画がある");

    assert_eq!(plan.stderr.filter.to_string(), "off", "端末は黙る");
    assert_eq!(file.filter.to_string(), flag_filter_text(2), "ファイルは -vv どおり DEBUG まで");
    assert!(plan.warning.is_none());
  }

  #[test]
  fn plan_keeps_the_shared_directive_even_when_quiet() {
    let plan = build_log_plan(None, 1, true, true);

    assert_eq!(plan.directive, flag_directive(1), "実行記録には端末の off ではなく共通の directive を書く");
  }

  #[test]
  fn quiet_and_verbose_without_log_file_is_harmless() {
    let plan = build_log_plan(None, 2, true, false);

    assert_eq!(plan.stderr.filter.to_string(), "off");
    assert!(plan.file.is_none());
    assert!(plan.warning.is_none(), "効果が無いだけで警告しない");
  }

  #[test]
  fn override_warning_survives_quiet() {
    let plan = build_log_plan(Some("info"), 1, true, true);
    let file = plan.file.expect("--log-file 指定時はファイル側の計画がある");

    assert_eq!(plan.stderr.filter.to_string(), "off");
    assert_eq!(file.filter.to_string(), "info");
    assert!(plan.warning.is_some(), "端末が黙っていても無視の警告はログファイルへ残す");
  }

  #[test]
  fn target_is_shown_only_from_trace() {
    for verbose in 0..=2 {
      assert!(
        !build_log_plan(None, verbose, false, false).stderr.show_target,
        "-v{verbose} 相当では target を表示しない"
      );
    }

    assert!(build_log_plan(None, 3, false, false).stderr.show_target, "-vvv では target を表示する");
  }

  #[test]
  fn rust_log_trace_shows_target() {
    assert!(build_log_plan(Some("seiran_compiler=trace"), 0, false, false).stderr.show_target);
    assert!(!build_log_plan(Some("info"), 0, false, false).stderr.show_target);
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
  fn rendered_report_includes_related_leaves() {
    // Arrange — `CompileFailure` は 2 件目以降を `related` に載せるので、同じ形の診断で全 leaf が残ることを見る
    let diagnostic = TestPrimary {
      rest: vec![TestRelated, TestRelated],
    };

    // Act
    let rendered = render_diagnostic_plain(&diagnostic);

    // Assert
    assert!(!rendered.contains('\u{1b}'), "致命的エラーも装飾なし");
    assert!(rendered.contains("cli::test_primary"), "主診断の code が残る");
    assert!(rendered.contains("主診断のヘルプ"), "主診断の help が残る");
    assert_eq!(rendered.matches("cli::test_related").count(), 2, "関連診断は件数ぶん全部残る");
    assert_eq!(rendered.matches("関連診断のヘルプ").count(), 2, "関連診断の help も残る");
  }

  #[test]
  fn filter_warning_renders_as_a_warning_with_its_code() {
    let rendered = render_diagnostic_plain(&FilterWarning::OverridesVerbose {
      value: String::from("error"),
    });

    assert!(rendered.contains("cli::rust_log::overrides_verbose"), "code が出る: {rendered}");
    assert!(rendered.contains("RUST_LOG=error"), "覆った値が出る: {rendered}");
    assert!(rendered.contains('⚠'), "warning として描かれる: {rendered}");
  }

  #[test]
  fn terminal_rendering_of_a_borrowed_diagnostic_matches_the_report() {
    // Arrange — 変更前の端末描画は `Report` の `Debug` だった
    let expected = format!("{:?}", miette::Report::new(TestWarning));

    // Act
    let rendered = format!("{:?}", TerminalDiagnostic(&MietteHandler::new(), &TestWarning));

    // Assert — 借用から描いても端末へ出るバイト列は変わらない
    assert_eq!(rendered, expected);
  }

  #[test]
  fn run_header_lists_what_the_log_is_a_record_of() {
    let header = RunHeader {
      subcommand: "build",
      base_dir: Some(Path::new("/work/book")),
    };

    let text = run_header_text("2026-09-13T10:00:00+09:00", &header, "warn");

    assert_eq!(
      text,
      format!(
        "# seiran 実行記録\n開始時刻: 2026-09-13T10:00:00+09:00\nバージョン: {}\nサブコマンド: build\n基準ディレクトリ: \
         /work/book\n実効フィルタ: warn",
        env!("CARGO_PKG_VERSION")
      )
    );
  }

  #[test]
  fn run_header_marks_an_unknown_base_dir() {
    let header = RunHeader {
      subcommand: "build",
      base_dir: None,
    };

    let text = run_header_text("t", &header, "warn");

    assert!(text.contains("基準ディレクトリ: （取得できませんでした）"), "{text}");
  }

  #[test]
  fn run_footer_states_the_outcome() {
    assert_eq!(run_footer_text("t", true), "# seiran 実行終了\n終了時刻: t\n終了状態: 成功");
    assert_eq!(run_footer_text("t", false), "# seiran 実行終了\n終了時刻: t\n終了状態: 失敗");
  }
}
