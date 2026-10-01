# ブロック設置・破壊 実装計画（M1-15）

> **Status**: 実装済み（2 クライアントでの実機確認待ち）— 2026-10-01。実機確認は `cargo run -p lodeframe --example blocks`

要件: REQ-API-002（ブロック部分）。決定: D9・D16・D21（[decisions.md](../decisions.md)）。
protocol（パケット型）と本体（`World` のチャンク編集・配信・イベント）にまたがるため、ここに設計を置く。

## 調べたこと

26.3 の server.jar の codec にパケットを実際に encode させて確かめた（`Bootstrap` 済みの JVM で、`Block.getId` まで確認）。

| 事実 | 影響 |
|---|---|
| `PlayerAction`（serverbound 41）= action（VarInt）、BlockPos（packed i64）、direction（**1 バイト**）、sequence（VarInt）。action は START_DESTROY_BLOCK 0・**CHANGE_DESTROY_DIRECTION 1**・ABORT 2・STOP 3・DROP_ALL 4・DROP 5・RELEASE_USE 6・SWAP 7・STAB 8 | 破壊は START（0）だけ扱う。26.3 で 1 に新しい action が入り、以前の並びとずれる |
| `UseItemOn`（66）= hand（VarInt）、BlockPos、direction（VarInt）、クリック位置（**ブロックからの相対 f32 ×3**）、inside（bool）、worldBorderHit（bool）、sequence（VarInt） | 置く位置は BlockPos + direction の向き |
| direction の並び = down 0・up 1・north 2・south 3・west 4・east 5（north は z−1、south は z+1） | `Direction` を protocol に置く |
| `BlockUpdate`（clientbound 8）= BlockPos、state（VarInt。STONE = 1） | 1 ブロックにつき 1 パケット |
| `BlockChangedAck`（4）= sequence（VarInt） | クライアントの予測を確定させる。**拒否しても必ず返す**（返さないと予測した見た目が残る） |

## 設計

### protocol

- `Direction`（6 方向、`from_id`・`offset`）
- `packets::play` に `PlayerAction`・`UseItemOn`（serverbound）、`BlockUpdate`・`BlockChangedAck`（clientbound）

### 本体（`World`）

- `Chunks::get_mut` を足す。編集は読み込み済みのチャンクをその場で書き換える
- 公開 API: `World::block(pos)` と `World::set_block(pos, state)`。`set_block` は World の全員へ `BlockUpdate` を送る。lobby が地形を動かすのにも使う
- `BlockBreakEvent { player, pos, block, cancelled }`。`PlayerAction` の START_DESTROY_BLOCK で発火
- `BlockPlaceEvent { player, pos, face, block, cancelled }`。`UseItemOn` で発火。`pos` は置く先（クリックしたブロック + `face`）。**`block` の既定は石**。ハンドラが差し替える
- 既定動作: 破壊は空気に、設置は `block` に置き換え、全員へ `BlockUpdate`
- キャンセル・拒否: 操作した本人へ、その位置の**今の状態**を `BlockUpdate` で送り返す
- どの経路でも、本人へ `BlockChangedAck(sequence)` を最後に送る
- 拒否: ワールドの高さの外、未ロードのチャンク（`ChunkLoader` が `None`）、破壊先がすでに空気
- ゲームモードは入室時に creative 固定（M1-11）。破壊は START 1 回で即時に壊す

### v0.1 でやらなかったこと（後で対応する）

意図して外したものを、理由と受け皿つきで置く。`backlog.md` の「持ち越し」にも同じ項目がある。

| 項目 | 今の動き | なぜ外したか | 受け皿 |
|---|---|---|---|
| アイテム→ブロックの対応（`SetCreativeModeSlot`・`SetCarriedItem`・`ItemStack`） | 置くブロックは `BlockPlaceEvent.block` の既定（石）。ハンドラで決める | items の生成データとインベントリが v0.1 に無い。先取りすると datagen と計画書が膨らむ | v0.2 のインベントリ（REQ-MACRO-004 の周辺で設計）。そのとき `block` の既定をスロットから決める |
| 設置の検証: 到達距離・プレイヤーとの衝突・置換可能ブロック（草・水）・オフハンド | 見ない。クリックしたブロックの隣に置く | 衝突には当たり判定データが要る。v0.1 の lobby に要らない | `BlockPlaceEvent` をキャンセルして利用者が制限できる。既定の検証は v0.2 |
| 向きで決まる state（階段・ドア・半ブロック・原木の軸） | `block` の既定 state のまま | クリック位置と向きから state を決める規則が要る | v0.2 |
| survival の掘り（STOP_DESTROY_BLOCK・掘り時間・ツール・ドロップ） | creative の即時破壊だけ | ゲームモードが creative 固定 | ゲームモード対応と一緒に（v0.2 以降） |
| 視界による配信の絞り込み | World の全員へ送る | 位置同期・チャットと同じ。v0.1 は全員 | REQ-ENT-002 で視界ごと絞る |
| 変更の永続化 | メモリ上のチャンクだけ。`ChunkLoader` へ書き戻さない。チャンクは破棄されないので同じ run の間は残る | 保存形式（Anvil）が v0.3 | REQ-WORLD-003 |
| ライトの再計算 | 全 15 のまま | REQ-WORLD-004 | REQ-WORLD-004 |
| 隣接ブロックの更新（水・レッドストーン・支えを失う） | しない | vanilla の挙動は持たない（D13） | 利用者がハンドラで書く |
| 複数ブロックの一括送信（`SectionBlocksUpdate`） | 1 ブロック 1 パケット | `set_block` の連続呼び出しが多くなったら | M1-18 の計測で問題になったら |

## テスト

`test-util` のハーネスで書く。注入は `TestEnv::send` に `PlayerAction` / `UseItemOn` を渡す。

- 破壊: 全員へ `BlockUpdate`（空気）、本人へ `BlockChangedAck`、`World::block` が空気
- 設置: クリックしたブロックの `face` 側に石が置かれ、全員へ届く。6 方向すべてで位置が合う
- キャンセル: 誰にも配られず、本人へ元の状態の `BlockUpdate` と `BlockChangedAck`
- 改変: ハンドラが差し替えたブロックが置かれる
- 拒否: 高さの外・未ロード・空気の破壊でも `BlockChangedAck` が返り、切断されない
- `set_block` が全員へ届く
- START 以外の action（drop 等）は何も起こさない

## 実機で確かめる

- 2 クライアントで、片方が壊した・置いたブロックがもう片方に見える
- ハンドラで置くブロックを別のものに差し替える（example）
- キャンセルしたとき、置いた（壊した）見た目が元に戻る
