//! 工程（phase）の開始と、結果付きの終了の記録（#551）
//!
//! 工程の入れ子は span が表す（#500）。この module は、その span の中で「工程を開始」と
//! 「工程を終了」（`status` と `elapsed`）の 2 つの INFO event を出す唯一の site を持つ。事実（件数）は
//! 引き続き各工程の callee が完了 event で出し、所要時間は終了 event だけが持つ（1 事象 1 オーナー）。
//!
//! 終了の状態は「[`Phase::succeed`] が呼ばれたか」だけで決まる。`?` の早期 return・失敗を返す
//! `return`・panic の unwind はどれも `succeed` を通らないので、失敗した工程も必ず終了 event を持つ
//! — ログから「どの工程で止まったか」を読めるようにするのがこの記録の目的だから。
//!
//! CLI の `render` / `write` は `seiran` crate の同名 module が同じ契約で記録する（target を各 crate に
//! 保つため、この型を facade へは載せない）。

use std::time::Instant;

use tracing::{Span, info, span::EnteredSpan};

/// 工程の終わり方。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PhaseStatus {
  /// 工程が成果を返した
  Succeeded,
  /// 工程が成果を返さずに終わった（失敗の返却・panic）
  Failed,
}

/// 1 工程の記録。生成で開始を、drop で終了を記録する。
///
/// 工程の span を保持するので、この値が生きている間の event はすべて工程の prefix を持つ。
#[must_use = "drop した時点で工程の終了を記録するので、工程の処理が終わるまで束縛しておく"]
pub(crate) struct Phase {
  /// 工程の span（drop で抜ける）。値を読まず保持するだけなので `_` 始まり
  _span: EnteredSpan,
  /// 開始時刻
  started: Instant,
  /// 現時点での終わり方（[`Phase::succeed`] まで `Failed`）
  status: PhaseStatus,
}

impl Phase {
  /// `span` に入り、工程の開始を記録する。
  ///
  /// span は呼び出し側の `info_span!("input")` 等で作る — span の名前と target は callsite で決まり、
  /// ここで作ると全工程の target がこの module になってしまう。一方「工程を開始」「工程を終了」の
  /// event 自身の target はこの module（`seiran_compiler::phase`）になる。そのため
  /// `RUST_LOG=seiran_compiler::typeset=info` のように工程の module 単位で絞ると、この開始・終了
  /// event は通らない — 見るには `seiran_compiler::phase=info` を directive へ足す。
  pub(crate) fn enter(span: Span) -> Self {
    let span = span.entered();
    info!("工程を開始");
    return Phase {
      _span: span,
      started: Instant::now(),
      status: PhaseStatus::Failed,
    };
  }

  /// 工程が成果を返したことを記録し、終了する。
  pub(crate) fn succeed(mut self) { self.status = PhaseStatus::Succeeded; }
}

impl Drop for Phase {
  /// 工程の終了を記録する。
  ///
  /// `drop` の本体はフィールド（`_span`）の drop より先に走るので、終了 event は工程の span の中で出る。
  fn drop(&mut self) {
    info!(status = ?self.status, elapsed = ?self.started.elapsed(), "工程を終了");
  }
}

#[cfg(test)]
mod tests {
  use std::{
    io::{self, Write},
    panic::{self, AssertUnwindSafe},
    sync::{Arc, Mutex},
  };

  use tracing::info_span;
  use tracing_subscriber::fmt::MakeWriter;

  use super::Phase;

  /// 書かれた内容を後から検査できる、複製可能な書き出し先。tracing の layer へそのまま渡せるよう
  /// `MakeWriter` も実装する（自身の複製を返す。`tests/trace_events.rs` の `CapturedLog` と同じ形）。
  #[derive(Clone)]
  struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

  impl Write for SharedBuffer {
    #[expect(
      clippy::unwrap_in_result,
      reason = "テスト専用の Mutex で他スレッドから触られることはなく毒されないため、panic し得ない"
    )]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
      self.0.lock().expect("テスト内でロックが毒されることはない").extend_from_slice(buf);
      return Ok(buf.len());
    }

    fn flush(&mut self) -> io::Result<()> { return Ok(()) }
  }

  impl<'writer> MakeWriter<'writer> for SharedBuffer {
    type Writer = Self;

    fn make_writer(&'writer self) -> Self::Writer { return self.clone(); }
  }

  /// `fmt` subscriber を thread-local（`set_default`）で張り、その中で `run` を実行して、捕捉したログを返す。
  fn capture_probe_log(run: impl FnOnce()) -> String {
    let buffer = SharedBuffer(Arc::new(Mutex::new(Vec::new())));
    let subscriber = tracing_subscriber::fmt()
      .compact()
      .with_max_level(tracing::Level::TRACE)
      .with_writer(buffer.clone())
      .with_ansi(false)
      .without_time()
      .finish();

    let guard = tracing::subscriber::set_default(subscriber);
    run();
    drop(guard);

    let written = buffer.0.lock().expect("テスト内でロックが毒されることはない").clone();
    return String::from_utf8(written).expect("UTF-8 のはず");
  }

  #[test]
  fn panicking_phase_records_a_failed_end() {
    // Arrange / Act — panic メッセージがテストの素の stderr に出ることは避けられない（`set_hook` は
    // プロセス全体で共有されるグローバル状態なので、他のテストを壊さないよう変更しない）。cargo test の
    // 既定の出力捕捉により、このテストが失敗しない限り表示されない。
    let log = capture_probe_log(|| {
      let unwound = panic::catch_unwind(AssertUnwindSafe(|| {
        let _phase = Phase::enter(info_span!("probe"));
        panic!("わざと落として Phase の Drop 経路を確かめる");
      }));
      assert!(unwound.is_err(), "panic が起きるはず");
    });

    // Assert
    assert!(
      log.lines().any(|line| {
        return line.contains("probe:")
          && line.contains("工程を終了")
          && line.contains("status=Failed")
          && line.contains("elapsed=");
      }),
      "panic した工程は status=Failed の終了 event を持つはず: {log}"
    );
  }

  #[test]
  fn succeeding_phase_records_a_succeeded_end() {
    // Arrange / Act
    let log = capture_probe_log(|| {
      let phase = Phase::enter(info_span!("probe"));
      phase.succeed();
    });

    // Assert
    assert!(
      log.lines().any(|line| {
        return line.contains("probe:")
          && line.contains("工程を終了")
          && line.contains("status=Succeeded")
          && line.contains("elapsed=");
      }),
      "succeed した工程は status=Succeeded の終了 event を持つはず: {log}"
    );
  }
}
