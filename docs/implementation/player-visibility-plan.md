# プレイヤー表示・移動同期 実装計画（M1-13）

> **Status**: 実装済み（2 クライアントでの実機確認待ち）— 2026-09-30

要件: REQ-ENT-001。決定: D6・D13（[decisions.md](../decisions.md)）。
protocol（パケット型）と本体（`World` の配信）にまたがるため、ここに設計を置く。

## 調べたこと

26.3 の server.jar の codec にパケットを実際に encode / decode させて確かめた（bytecode を読むだけでは足りなかった箇所）。

| 事実 | 影響 |
|---|---|
| `PlayerInfoUpdate` = アクション集合（**1 バイトのビットマスク**、ordinal 順: ADD_PLAYER 0・INITIALIZE_CHAT 1・UPDATE_GAME_MODE 2・UPDATE_LISTED 3・UPDATE_LATENCY 4・…）、VarInt 個数、各エントリ = UUID + **立っているアクションの順に**値（ADD_PLAYER = 名前 + プロパティ列、GAME_MODE = VarInt、LISTED = bool、LATENCY = VarInt） | v0.1 は ADD_PLAYER・GAME_MODE・LISTED・LATENCY（0x1D）に固定 |
| `PlayerInfoRemove` = UUID の VarInt 長リスト | |
| `AddEntity` = id（VarInt）、UUID、種別（VarInt）、x・y・z（f64）、**速度（低精度ベクトル。0 は 1 バイトの 0x00）**、pitch・yaw・head_yaw（各 1 バイトの角度 = 度 × 256 / 360）、data（VarInt） | 速度は 0 のみ対応。プレイヤーの種別 ID は 159 |
| `EntityPositionSync`（26.3 は `PositionPath` を含む）= id（VarInt）、**path 種別 0x00**、x・y・z（f64）、yaw・pitch（f32）、on_ground | 移動は差分（MoveEntity）を使わず毎回これで送る |
| `RotateHead` = id（VarInt）+ head_yaw（1 バイト角度）。`RemoveEntities` = VarInt 長の id リスト | |
| `SetEntityData` = id（VarInt）+ 要素列（index u8、型 u8、値）+ 終端 0xFF。フラグ = index 0・型 BYTE(0)・スニークは bit 1。ポーズ = index 6・型 POSE(20)・VarInt（CROUCHING = 5） | スニークはこの 2 要素を送る |
| `PlayerInput`（serverbound 43）= 1 バイトのビット（forward 1・backward 2・left 4・right 8・jump 0x10・**shift 0x20**・sprint 0x40） | スニークの検出はこれ |

## 設計

### protocol

`packets::play` に上を足す。`SetEntityData` はスニークに要る 2 要素に限った型（一般のメタデータは v0.2 の ENT-002）。

### 本体（`World`）

`Player` に `entity_id`・`name`・向き・`on_ground`・`sneaking` を持たせ、同じ World の他プレイヤー全員へ配る。視界による絞り込みはしない（v0.1 は全員に送る。ENT-002 で視界へ）。

- 入室: 本人へ全員の `PlayerInfoUpdate` と既存プレイヤーの `AddEntity`。既存プレイヤーへ本人の `PlayerInfoUpdate` と `AddEntity`
- 移動・視線（`MovePlayer*`）: 他へ `EntityPositionSync` + `RotateHead`。位置・向きが変わらないパケットは送らない
- スニーク（`PlayerInput`）: 他へ `SetEntityData`
- 退出: 他へ `RemoveEntities` と `PlayerInfoRemove`

### v0.1 でやらないこと

- 差分移動（`MoveEntity*`）による帯域節約。ボット N 体の計測（M1-18）で問題になったら足す
- スキン。オフラインモードはプロパティ空
- 入室時の `SetEntityData`（すでにスニーク中のプレイヤーの見た目）

## 実機で確かめる

- 2 クライアントが互いを視認し、移動・視線・スニークが同期する
- タブリストへの追加と退出時の削除
