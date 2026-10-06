//! 工程（phase）の開始と、結果付きの終了の記録
//!
//! 工程の入れ子は span が表す。この module は、その span の中で「工程を開始」と
//! 「工程を終了」（`status` と `elapsed`）の 2 つの INFO event を出す唯一の site を持つ。
//!
//! 終了の状態は「[`Phase::succeed`] が呼ばれたか」だけで決まる。`?` の早期 return・失敗を返す
//! `return`・panic の unwind はどれも `succeed` を通らないので、失敗した工程も必ず終了 event を持つ。

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
  /// 工程の span（drop で抜ける）
  _span: EnteredSpan,
  /// 開始時刻
  started: Instant,
  /// 現時点での終わり方（[`Phase::succeed`] まで `Failed`）
  status: PhaseStatus,
}

impl Phase {
  /// `span` に入り、工程の開始を記録する。
  ///
  /// span は呼び出し側の `info_span!("input")` 等で作る（span の名前と target は callsite で決まる）。
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
    thread,
  };

  use tracing::{Dispatch, Span, Subscriber, info_span, subscriber::NoSubscriber};
  use tracing_subscriber::{Layer, filter::LevelFilter, fmt::MakeWriter};

  use super::Phase;

  /// 書かれた内容を後から検査できる、複製可能な書き出し先。`MakeWriter` は自身の複製を返す。
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
  ///
  /// subscriber の無い別スレッドが同じ callsite を先に通っても、`run` の event は捕捉される。
  fn capture_probe_log(run: impl FnOnce()) -> String {
    let buffer = SharedBuffer(Arc::new(Mutex::new(Vec::new())));
    let subscriber = tracing_subscriber::fmt()
      .compact()
      .with_max_level(tracing::Level::TRACE)
      .with_writer(buffer.clone())
      .with_ansi(false)
      .without_time()
      .finish();

    // tracing-core は登録済みの dispatcher が 1 つだけのとき、callsite の interest を「その callsite を最初に
    // 通ったスレッドの thread-local default」だけで決めて固定する（`tokio-rs/tracing#3611`）。subscriber の無い
    // スレッドが先に通ると `never` が固定され、このスレッドの event も捨てられる。どこにも張らない dispatcher（peer）を
    // 捕捉の間生かして 2 つ以上にしておくと、interest は生きている全 dispatcher の合成（食い違えば `sometimes`）に
    // なり、event ごとに各スレッドの default へ `enabled` が問われる。peer の最大レベルを OFF にして先に登録し、
    // 最大レベルを上げる（＝他のスレッドが callsite を登録し始める）のを捕捉用の登録だけにする — その登録の時点で
    // dispatcher は既に 2 つある。
    let peer = LevelFilter::OFF.with_subscriber(NoSubscriber::default());
    assert_eq!(peer.max_level_hint(), Some(LevelFilter::OFF), "peer は最大レベルを上げないはず");
    let peer = Dispatch::new(peer);
    let guard = tracing::subscriber::set_default(subscriber);
    run();
    drop(guard);
    drop(peer);

    let written = buffer.0.lock().expect("テスト内でロックが毒されることはない").clone();
    return String::from_utf8(written).expect("UTF-8 のはず");
  }

  #[test]
  fn panicking_phase_records_a_failed_end() {
    // panic メッセージがテストの素の stderr に出ることは避けられない（`set_hook` は
    // プロセス全体で共有されるグローバル状態なので、他のテストを壊さないよう変更しない）。cargo test の
    // 既定の出力捕捉により、このテストが失敗しない限り表示されない。
    let log = capture_probe_log(|| {
      // 並列に走る他のテストのスレッド（subscriber なし）が `Phase` の callsite を先に通る状況を、
      // 捕捉の開始後（最大レベルが上がり callsite が登録されうる時点）に必ず起こす
      thread::spawn(|| {
        drop(Phase::enter(Span::none()));
      })
      .join()
      .expect("subscriber の無いスレッドで工程を開始・終了しても panic しないはず");

      let unwound = panic::catch_unwind(AssertUnwindSafe(|| {
        let _phase = Phase::enter(info_span!("probe"));
        panic!("わざと落として Phase の Drop 経路を確かめる");
      }));
      assert!(unwound.is_err(), "panic が起きるはず");
    });

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
}
