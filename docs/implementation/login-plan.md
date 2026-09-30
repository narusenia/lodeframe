# offline login と configuration 実装計画（M1-08）

> **Status**: 実装済み（レビュー待ち）— 2026-09-30。vanilla 26.3 クライアントで Play 到達を確認

要件: REQ-NET-003。決定: D4・D5（[decisions.md](../decisions.md)）。
xtask・protocol・本体にまたがり、レジストリの公開 API を決めるためここに設計を置く。

## 問題

vanilla クライアントを Handshake → Login → Configuration → Play まで通す。
Configuration ではレジストリ（dimension type、biome 等）をクライアントに渡す必要がある。
クライアントが同じ版の vanilla データを持つので、Known Packs で「`minecraft:core` は持っているはず」と
合意すれば、エントリ本体（NBT）は省いて**名前だけ**送れる。利用者が足す・差し替えるエントリだけ NBT を付ける。

## 調べたこと（26.3 の server.jar、2026-09-30）

| 事実 | 影響 |
|---|---|
| `registries.json` は組み込みレジストリ（95 種）のみ。dimension_type・biome 等の datapack レジストリは含まない | 別の出力が要る |
| `--server` を付けた data generator が `data/minecraft/<registry>/<name>.json` を出す（0.8 秒、8,372 ファイル）。`datapack.json` は各レジストリが data 駆動かを示す | エントリ名はファイル名から取れる。生成物には**名前だけ**を commit し、JSON 本体は commit しない（D17） |
| どのレジストリが configuration で同期されるか（vanilla の `SYNCHRONIZED_REGISTRIES`）は出力に無い | server.jar を `javap -c -p` で読んで確定（32 種）。版を上げるたびに読み直す。足りないとクライアントが `Missing element` で切断する |
| LoginFinished は `gameProfile` の後ろに `sessionId: UUID` を持つ（26.x） | 手書きの配置は jar のコーデックと照合して起こす。実クライアントが最終確認 |
| クライアントは参照されるタグが無いと finish で切断する。tags は `data/minecraft/tags/` に出る | タグも生成する（静的は固定 ID、動的は送るレジストリの順で解決） |

## 分担

| 場所 | 変更 |
|---|---|
| `xtask datagen` | `--server` を追加で実行し、datapack レジストリごとのエントリ名（ソート済み）を `generated/datapack_registries.rs` に出す |
| `lodeframe-protocol` | `packets::{login, configuration}`。offline UUID（`OfflinePlayer:<name>` の MD5、version 3）。`Uuid` 型は既存 |
| `lodeframe` | `login`（offline ログインと圧縮通知）、`configuration`（Known Packs → レジストリ → 完了）、`Registries`（利用者が追加・差し替える API） |

## フロー（offline）

1. Handshake（M1-06）→ Login へ
2. クライアント `Hello`（名前・UUID）→ サーバー `LoginCompression`（任意）→ `LoginFinished`（offline UUID・名前・空のプロパティ）
3. クライアント `LoginAcknowledged` → Configuration へ
4. クライアント `ClientInformation`（受け取って保存）。サーバー `SelectKnownPacks`（`minecraft:core`）→ クライアント `SelectKnownPacks`
5. サーバー `RegistryData`（レジストリごと。vanilla 分は NBT なし、利用者分は NBT 付き）、`UpdateTags`（空で足りるか実測）
6. サーバー `FinishConfiguration` → クライアント `FinishConfiguration` → Play へ

## `Registries`

- 既定は vanilla のエントリ名だけ（全同期レジストリ）。
- 利用者は `registries.set(registry, name, nbt)` で追加・差し替える（差し替えはエントリ名が既存なら NBT 付きで送る）。
- エントリの**並び順がネットワーク ID**になる。Known Packs の有無にかかわらずサーバーが決めた順を以後の Play パケット（biome ID 等）でも使う。
- 起動時に確定し、以後は不変で共有する（architecture.md「グローバル可変シングルトンを置かない」）。

## 未検証

- `ClientInformation`・brand は読み捨てている（Play で要る時点で扱う）
- 版を上げたときの `SYNCHRONIZED_REGISTRIES` の読み直しは手作業

## 完了条件

- [x] offline UUID が既知の値（vanilla と同じ）と一致する（テスト）
- [x] 各パケットが手書きの配置どおりに往復する（テスト）
- [x] 利用者が追加・差し替えたエントリだけ NBT を送る（テスト）
- [x] vanilla 26.3 クライアントが offline mode でログインし Play に入る（実機。チャンク無しで落ちてよい）

## 非対象

online mode・暗号化（v0.1 外）、Velocity forwarding、Play 以降（M1-09 以降）、配布物のチャンク・スポーン（M1-10, M1-11）。
