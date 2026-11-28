# Code audit 2026-11-28 — 驗證結果

對 [`code-audit-2026-11-28-plan.md`](code-audit-2026-11-28-plan.md) 與九份區塊報告（`code-audit-2026-11-28-{a..i}.md`）的每一條 finding 做第二次獨立查證。本文件只記錄查證結論；**修正尚未開始**，修正排程見文末。

- **基準：** `dev` @ `e465942`（報告寫成時的 code；之後只有文件 commit）。遊戲 2.06，live install 在 `D:\SteamLibrary\...`，版本封存 `X:\Crimson Desert\gamedata-bin\`（2.05.01、2.06 含 `dlc_01` 的鬆散檔），fixtures 在 `D:\Github\crimson-rs-private\saves\`。
- **方法：** 每個區塊一個獨立的查證 agent，逐條開原始碼追路徑，不沿用原報告的推理；只讀，不建置。需要執行才能判定的項目由主 session 在這台機器上跑（暫時的 `#[ignore]` probe 模組、Python 唯讀腳本），probe 跑完即移除、不提交。
- **判定：** CONFIRMED（程式碼如所述、後果成立）／PARTLY（缺陷存在但位置、計數、後果、嚴重度或來源有誤）／REFUTED（缺陷不存在）／NEEDS-RUN（仍需執行才能判定）。

---

## 結論摘要

| 區塊 | findings | CONFIRMED | PARTLY | REFUTED | NEEDS-RUN |
|---|---:|---:|---:|---:|---:|
| A 封存容器與加密 | 19 | 16 | 2 | 1 | 0 |
| B 表頭／列頭佈局層 | 12 | 4 | 8 | 0 | 0 |
| C ItemInfo／Skill／StringInfo | 19 | 16 | 3 | 0 | 0 |
| D key-resolver 橋接 | 11 | 7 | 4 | 0 | 0 |
| E 染色／Prefab | 16 | 12 | 4 | 0 | 0 |
| F 存檔格式核心 | 14 | 11 | 3 | 0 | 0 |
| G 存檔編輯 C ABI | 16 | 13 | 2 | 0 | 1 |
| H Python 綁定與打包 | 19 | 17 | 2 | 0 | 0 |
| I 建置／CI／腳本 | 23 | 19 | 4 | 0 | 0 |
| **合計** | **149** | **115** | **32** | **1** | **1** |

PARTLY 多半是計數、行號、來源或後果的細節修正，缺陷本身存在；B 區塊 PARTLY 偏多，是因為 2.06 實際資料推翻了幾個「沒有真實表觸發」的前提。唯一 REFUTED 的是 A-07（`copy_nonoverlapping` 在 count 為 0 時允許 null）。

**最重要的幾件事：**

1. **NEW-1（新發現，medium）**：2.06 skill 有 78% 的 entry 靜默退成 raw fallback（2.05.01 是 14%），所有測試都綠燈。Python 呼叫端拿不到 buff 資料；C ABI 不受影響。
2. **D-01（medium，報告低估）**：2.06 `_with_header` loader 實際少掉 mission 2,444、stage 3,198、gimmick 140、quest 1 列，stage 還多出 2 個雜訊 key。這正是文件引導 CrimsonAtomtic 走的路徑。
3. **G-01（high）**：兩個列舉 API 不取 FFI lock，而 C# 宿主確實會在 `Task.Run` 上同時呼叫列舉與修改。
4. **F-01（high，潛在）**：已實測，改 1 byte 就能讓 `Body::write` 回 Ok 並靜默少寫 809 B。目前 30 份真實存檔都不觸發。
5. **CI 只跑 399 個測試中的 111 個**（A-14／D-03／E-08／I-01）。上面幾項都沒被 CI 抓到，所以排在修正的第一步。

**嚴重度調整建議：** 調降 A-04、A-06、A-16、B-01、B-07、C-05、C-11、D-02、E-06、E-07、F-02、G-02（high→medium）、G-05、H-11；調升 C-03（→ medium），H-06、H-17 由 plausible 升為 confirmed。

## 修正狀態總表

每條 finding 一列，每批修正後更新。狀態：✅ 已修、🟡 部分、⬜ 未修、➖ 不修、↪ 併入他項。commit 是 `dev` 上的修正 commit；細節見 [`dev-log.md`](dev-log.md) 的 2026-11-28 各批條目。

<!-- status-table:start -->

| ID | 問題（摘要，取自原報告） | 嚴重度（查證後） | 狀態 | 修正 commit／備註 |
|---|---|---|---|---|
| A-01 | Trie `get_string` 無界遞迴：舊編碼可自我循環，新編碼可深鏈，都會 stack overflow 讓整個行程 abort | medium | ✅ 已修 | `9805e56` |
| A-02 | 依 PAMT 宣告值配置記憶體（`dir_count`、`file_count`、`compressed_size`、`uncompre… | medium | ✅ 已修 | `0cdfd25` |
| A-03 | `decompress` 不驗證輸出長度等於 PAMT 宣告值，zlib 也沒有上限（解壓縮炸彈） | medium | ✅ 已修 | `0cdfd25` |
| A-04 | `PackGroupBuilder` 用呼叫端的 `encrypt_info` 加密，PAMT 表頭卻寫死 `[2,14,97]`：Ch… | low | ✅ 已修 | `0d0b576`（加 ChaCha20 打包往返測試） |
| A-05 | 未知的 compression（5–15）與 crypto（4–15）nibble 被當成「不壓縮／不加密」，版本漂移不會明確失敗 | low | ✅ 已修 | `1c7023e` |
| A-06 | ChaCha20 用 32 位元區塊計數器並 `seek` 到檔名雜湊，計數器接近上限的大檔會在 `apply_keystream` p… | nit/low | ✅ 已修 | `0d0b576`（計數器溢位改回錯誤，不再 panic） |
| A-07 | `crimson_paz_extract_file` 在抽出 0 位元組、`out_buf` 為 null 時仍呼叫 `copy_non… | nit | ➖ 不修 | REFUTED |
| A-08 | PAMT 的 `chunk_offset + compressed_size` 是 u32 加法：debug panic，release… | low | ✅ 已修 | `1c7023e` |
| A-09 | PAPGT 舊版佈局的 `i32` 名稱長度為負時，`check_remaining` 的加法溢位，最後 panic | low | ✅ 已修 | `1c7023e` |
| A-10 | Roundtrip 不對稱：`COptional` flag ≥ 2、`CFlaggedArray` 的 `01`+count 0、pa… | low | 🟡 部分 | flag 非 0/1、`01`+count 0、paver 未知尾段改報錯 `0d0b576`；PAPGT 舊版尾端多餘位元組未改 |
| A-11 | `CArray` 的上限以「剩餘位元組數」計，不是「剩餘位元組 / 元素最小大小」，配置可達剩餘位元組 × `size_of::<T>()` | low | ✅ 已修 | `1c7023e` |
| A-12 | DDS per-mip 解碼器：`mip_count`／寬高沒有上限，算術在 release 下會 wrap，迴圈可達 2³² 次 | low | ✅ 已修 | `5fbd2fd` |
| A-13 | 版本閘門只有常數和文件公式，沒有 Rust 或 C ABI 函式實作「成對比較 + patch 下限」，也沒有測試釘住公式 | low | ✅ 已修 | `0d0b576`（`parser_supports` + C ABI `crimson_parser_supports` + 測試） |
| A-14 | A 區塊的關鍵測試在 CI 上不跑：CI 是 `cargo test --lib`（不含 `c_abi`），checksum KAT 只… | low | ✅ 已修 | CI 跑 `c_abi` 測試 `45daf6f`、解壓縮大小測試 `0cdfd25`、ChaCha20 KAT `0d0b576` |
| A-15 | Python-only builder 的截斷：PAPGT `entry_count` u8 wrap、trie 片段長度 `as u8… | low | ✅ 已修 | `0d0b576` |
| A-16 | PALOC handle：重複 key 時 lookup 與列舉不一致；非 UTF-8 字串每次載入都 `Box::leak` | nit | ✅ 已修 | `66d6245` |
| A-17 | Paver 的 DLC 尾段讀不懂時退回 10 位元組讀法，`build` 變成 DLC 數量；`BinaryRead for Pave… | nit | ✅ 已修 | `0d0b576` |
| A-18 | 文件與程式碼脫節：缺 `.paz` 回 `BODY_PARSE`（文件說 `IO`）、worldmap SDF 的舊說明、`c-abi-… | nit | 🟡 部分 | 符號目錄補齊 `e0a9689`；其他文件細項未改 |
| A-19 | Feature gate／lint 小問題：`papgt.rs` 對有用到的 import 標 `allow(unused_import… | nit | ⬜ 未修 |  |
| B-01 | `legacy_pair` / `to_legacy_layout` 失敗時靜默退回原始位元組，版本漂移不會明確失敗 | low | ↪ 併入 | → B-02 |
| B-02 | C ABI 的 `_with_header`、`_load_from_file`、name-only 巨集與 DLC merge 在 h… | medium | ✅ 已修 | 雜訊列隨 D-01 `a6857ac`；loader、`sibling_header`、DLC merge 改回報 `b14b6b4` |
| B-03 | test-only `gamedata_layout::resolved()` 找不到已知佈局時默默選最新佈局，live 測試會「ski… | low | ✅ 已修 | `d541ce2`（找不到佈局改為 panic） |
| B-04 | 寬度組合與錯誤路徑的單元測試覆蓋不足；`WIDTHS` 只有 15 種組合中的 12 種 | low | ⬜ 未修 |  |
| B-05 | 直接呼叫 `parse_named_header` 的 iteminfo 與 stringinfo 不驗證 offset+size 鏈；… | low | ✅ 已修 | `017f867`（iteminfo 加 packing 檢查；stringinfo 本就逐列比對 offset） |
| B-06 | 2.06 header 的 key 和 body 列首的 key 沒有交叉比對；skill 寫回時以 body key 為準 | low | ✅ 已修 | `017f867` |
| B-07 | Python 端兩份 2.06 header 讀取器支援的寬度與檢查都和 Rust `WIDTHS` 不同步 | nit | ⬜ 未修 |  |
| B-08 | 寬度一律自動偵測，不用呼叫端已知的寬度；第一個吻合的組合勝出，沒有歧義檢查 | low | ✅ 已修 | `b89496a`（偵測改為比對 header 與 body 的 key；未改成由呼叫端傳寬度） |
| B-09 | `from_legacy_layout` 沒檢查 `count_width` 的上限（>8 會 panic），而且找列尾是 O(n²) | nit | ✅ 已修 | `1c7023e` |
| B-10 | `EntrySources::next_merge` 在 255 飽和，失敗的 merge 也會用掉一個來源編號 | nit | ✅ 已修 | `b89496a` |
| B-11 | `impl_name_only_bridge!` 的小問題：`#[repr(C)]` 標在含 `HashMap` 的 handle 上、… | nit | ⬜ 未修 |  |
| B-12 | 文件與程式碼脫節：CLAUDE.md 的 skill `BString` 規則、`row_head.rs` 的 key 寬度說明、`c-… | nit | ⬜ 未修 |  |
| C-01 | `TailSizeCache` 多 GB 配置：根因已在 `a9d7012` 修正，但文件仍把它當成現存問題，而且「HashMap 順序… | low | ✅ 已修 | 文件更新 `7426d4b` |
| C-02 | ItemInfo 沒有任何讀 live install 的位元組 roundtrip 測試，DLC 表連一次寫回都沒有測 | medium | ✅ 已修 | `02395c6` |
| C-03 | Skill 的 raw fallback 是靜默降級，測試只印出 fallback 數量、不檢查 | medium | ✅ 已修 | `475b3fa`（NEW-1 一起修，live 測試釘 20% 上限） |
| C-04 | Skill post-buff 驗證器的硬上限（list ≤ 50、字串 ≤ 2000、必須是 UTF-8）一旦被真實資料超過，整張表就… | low | ✅ 已修 | `59adc42`（上限保留為常數，live 測試在最大值超過一半時 fail；2.06：list 13／50、字串 349／2000） |
| C-05 | `detect_format` 在範圍檢查之前切片，舊版 header 的 offset 超過 body 時會 panic | nit | ✅ 已修 | `1c7023e` |
| C-06 | `crimson_iteminfo_load_from_bytes` 在 2.06 上回傳 `OK` 但名稱全空，`lookup_str… | low | ✅ 已修 | `b89496a` |
| C-07 | 只有 body-only 的 iteminfo 路徑會吃到 A-11 的配置放大（有 header 的路徑以列長為上限） | low | ✅ 已修 | `1c7023e`（隨 A-11） |
| C-08 | `parse_iteminfo_with_header` 不檢查列是否緊密排列、是否涵蓋整個 body，C ABI 又跳過尾端檢查 | low | ✅ 已修 | `017f867` |
| C-09 | Skill 不比對 header key 和列內 key；2.06 重建時直接丟掉 header 的 key | low | ✅ 已修 | `017f867` |
| C-10 | 存在旗標「非 0 即存在」：讀到未知值時猜測後繼續，寫回時正規化成 `1`；條件欄位在修改後不對稱 | low | ✅ 已修 | `0d0b576`；條件欄位寫回檢查 `1a41191`（隨 H-12） |
| C-11 | StringInfo 的 C ABI 只看 body，`RowLayout::detect` 以「第一個 u32 是否為 0」判斷佈局 | nit | ⬜ 未修 |  |
| C-12 | `artifact_by_mission` 在 1:1 不變式被打破時，誰勝出取決於 `HashMap` 的隨機迭代順序；DLL 往 s… | nit | ✅ 已修 | `b89496a`（最小 ItemKey 勝出，不寫 stderr） |
| C-13 | `CrimsonItemInfoSummary` 只釘大小和對齊、沒有釘欄位 offset；結構、flag 位元與 19 個 scala… | low | 🟡 部分 | offset 全數 build-time 釘住、註解修正 `7426d4b`；結構與 flag 仍未寫進 docs |
| C-14 | `docs/c-abi-surface.md` 沒有逐一列出這三個 bridge 的 `extern "C"` 符號 | low | ✅ 已修 | `e0a9689` |
| C-15 | 過時與自相矛盾的註解（目錄名稱、版本、gem 定義、slot 數、BString） | nit | ⬜ 未修 |  |
| C-16 | `quick_slot_index` 從 2.04 起恆為 0，getter 卻回 `OK`；`IS_IMPORTANT_ITEM` 是… | nit | ⬜ 未修 |  |
| C-17 | `item_info` / `string_info` 無條件的 `allow(dead_code)` 抵銷了 `lib.rs` 依賴的… | nit | ✅ 已修 | `423d0a1` |
| C-18 | `test_skill_roundtrip_cross_version` 在封存存在時必定失敗；`test_skill_roundtri… | low | ✅ 已修 | `02395c6`；資料改放 private repo `5ad3a14` |
| C-19 | 非 UTF-8 的 `CString` 會 `Box::leak`，在長駐的 C# 編輯器裡每次載入都洩漏 | nit | ✅ 已修 | `66d6245` |
| D-01 | 2.06 的 `_with_header`／`_load_from_file` 路徑丟掉 header 的權威列表，改用啟發式掃描重建的… | medium | ✅ 已修 | `a6857ac` |
| D-02 | characterinfo 在 2.06 載入了 359 個雜湊 32-bit key 列，但 `lookup_string_key`／… | low | ✅ 已修 | `a6857ac` |
| D-03 | CI 用 default features 跑 `cargo test --lib`，D 區塊全部模組都在 `c_abi` 之後，離線單… | medium | ✅ 已修 | `45daf6f` |
| D-04 | gimmick、character 的 `lookup_display_name` 不檢查 key 是否在表內，任何 u32 都直接組… | low | ➖ 不修 | PALOC 依 key 查名稱也涵蓋只在 DLC 的列，限制在表內會漏掉它們 |
| D-05 | knowledge／characterinfo 的 DLC merge 收到空 header 時，會用 body-only 掃描讀 2.… | low | ✅ 已修 | `b89496a`（空 header 回 `BODY_PARSE`） |
| D-06 | 固定偏移量的欄位抽取（faction group、factionnode knowledge、character faction、glo… | low | 🟡 部分 | faction 列太短改為整表失敗 `950aca9`（`[u8; 3]` 並非全為 0，零值檢查不可行）；factionnode／character／global game event／relation group 未改 |
| D-07 | live 測試的列數 pin 太寬（store `>=250` 但實際 441、mission `>1000` 但實際 6,872 列）… | low | 🟡 部分 | 七張表改比對 header 列數 `a6857ac`；其他表的寬鬆 pin 未改 |
| D-08 | `globalgameevent` 的 `paloc_key` 只接受剛好 14 位數；註解說「event_key < 0x4300」，… | nit | ✅ 已修 | `7426d4b` |
| D-09 | `faction_info` 手寫版和巨集版的小差異：`get_entry` 沒先清 `*out_key`；safe fn `slice… | nit | ✅ 已修 | `9e73dd9` |
| D-10 | 策展表的 drift 檢查只有單向；`live_titles` 用 last-wins 建 map，和 bridge 的 first-w… | nit | ⬜ 未修 |  |
| D-11 | 文件與程式碼脫節：`c-abi-surface.md` 沒有逐一列出 D 區塊的符號、列數過時（計畫書待查事項）、巨集說明、模組文件互相… | nit | 🟡 部分 | 第 1 項（符號）`e0a9689`；其餘文件細項未改 |
| E-01 | `resolve_dye_slot_count` 回傳的是第一個 mesh 的 sub-mesh 數，不是 dye slot 數；rus… | medium | ✅ 已修 | `b90dfcf` |
| E-02 | Dye 色票的「等長切分」fallback 會在長度混雜但總長剛好整除時靜默讀錯，違反模組文件「讀不懂就給空色票」的承諾 | low | ✅ 已修 | `1a41191` |
| E-03 | `palette_knowledge_at` 的 live 測試把 `UNSUPPORTED_VERSION` 當成「舊版 instal… | low | ✅ 已修 | `4213732` |
| E-04 | 四個 E loader 遇到版本漂移都回 `OK`，只給空的或部分的 handle；C ABI 沒有任何「被丟棄的列數」訊號 | low | 🟡 部分 | 表頭讀不懂、整表讀不到改回 `BODY_PARSE` `b14b6b4`；表內單列被丟棄仍無訊號（live 測試釘列數） |
| E-05 | Prefab join 把抽取錯誤當成「沒有這個 prefab」吞掉，並快取結果；DLC merge 會退回同名的 base prefa… | low | 🟡 部分 | 抽取錯誤改為回報、不快取、不退回 base `1a41191`；merge 仍用載入時解析的 PAMT |
| E-06 | `.prefab` 字串掃描與路徑正規化：`.pac` 副檔名區分大小寫、排除空白與非 ASCII、不處理 `\`、同名 prefab… | nit | ⬜ 未修 |  |
| E-07 | 新的 `_with_prefab_pamt` 與 `merge_dlc` 入口會解析呼叫端給的 PAMT，繼承 A-01／A-02 的行… | nit | ↪ 併入 | → A-01／A-02 |
| E-08 | E 區塊的測試（包括不需要 install 的合成測試）在 CI 上一個都不跑 | low | ✅ 已修 | `45daf6f` |
| E-09 | `crimson_item_part_prefab_lookup_dye_slot_material` 的 `dye_slot_no`… | nit | ⬜ 未修 |  |
| E-10 | 重複 key 的處理不一致：三個表 handle 都取第一筆，`item_part_prefab` 的 `rows` 取最後一筆 | nit | ⬜ 未修 |  |
| E-11 | 對 name-join handle 呼叫 `merge_dlc` 時，文件說會忽略 `dlc_prefab_pamt_path`，實際… | nit | ⬜ 未修 |  |
| E-12 | 回傳碼語意重疊：`NOT_FOUND` 同時代表「沒這個 key」和「這格不需要解鎖」等；非 UTF-8 的 tribe 字串回 `IN… | nit | ⬜ 未修 |  |
| E-13 | 已知是常數的位元組沒有驗證（slot marker `0xFF`、row pad、palette pad），降低了偵測漂移的能力 | nit | ⬜ 未修 |  |
| E-14 | 文件與程式碼脫節：列數、2.06 之前的欄位說明、`key=0` 的 sub 數、錯誤碼清單、`resolved_item_count`… | nit | 🟡 部分 | 符號名稱 `e0a9689`；其餘文件細項未改 |
| E-15 | FFI 衛生：opaque handle 標 `#[repr(C)]`、私有 safe fn 直接解參考 raw pointer、部分… | nit | ⬜ 未修 |  |
| E-16 | 測試小問題：測試裡的 `1 << n` 可能在 assert 前溢位；key 不變式的測試不必要地依賴 0009 PAMT | nit | ⬜ 未修 |  |
| F-01 | Encoder 不拒絕 decoder 沒解釋完的 block（undecoded 位元組、`Unknown` 欄位、placehold… | high | ✅ 已修 | `ecb309e` |
| F-02 | TOC `toc_count` 直接 `Vec::with_capacity`：`0xFFFFFFFF` 要約 128 GiB，配置失敗… | low | ✅ 已修 | `5da56e3` |
| F-03 | Locator / object list 解碼無深度上限、無循環偵測：可 stack overflow（abort），而且多重回溯造成… | medium | ✅ 已修 | `5da56e3` |
| F-04 | 存檔核心的關鍵測試幾乎只讀 `%LOCALAPPDATA%` 的 live save，不讀 fixtures；`src/save/` 沒… | medium | 🟡 部分 | round-trip 測試加入 fixtures `d541ce2`、F-01 fixture 測試 `ecb309e`；原始位元組比對未加 |
| F-05 | `uncomp_size` 未驗證就交給 `lz4_flex`，safe-decode 會先 `vec![0; uncomp_size]… | low | ✅ 已修 | `0cdfd25` |
| F-06 | ChaCha20 用 nonce 前 4 位元組當 32 位元計數器，計數器跨過 2³² 時 `apply_keystream` pan… | low | ✅ 已修 | `0d0b576`（隨 A-06；遊戲端進位語意仍未知，溢位時報錯） |
| F-07 | 構造輸入可觸發的 panic：marker 走訪呼叫 `try_decode_object_list_at` 前沒檢查 18 位元組；`… | low | ✅ 已修 | `1c7023e` |
| F-08 | `trailing_pad` 內的 stale offset 只在「整個 block 搬家、內部不變長」時才會被 relocate；同一… | low | ✅ 已修 | `202c8ec`（長度改變又帶 pad 的 block 拒絕重新編碼；wrapper prefix 在所有存檔都是單一 `01`） |
| F-09 | 版本漂移不會明確失敗：未知 `meta_kind`、mask byte count 超界、未知 class、TOC 截短、list 部分… | low | 🟡 部分 | TOC 截短改報錯 `5da56e3`；其餘「猜了繼續讀」未改 |
| F-10 | HMAC 驗證失敗仍可載入，寫回時一律重算 HMAC 而不看 `hmac_ok`：損壞或被竄改的存檔會被重新封成「有效」 | low | ✅ 已修 | `b14b6b4`（寫回回 `HMAC_UNVERIFIED` −28，`crimson_save_set_allow_hmac_rewrap` 可明確放行） |
| F-11 | Header / TOC 的 offset 與 size 只做到「不越界」，沒驗證順序、重疊、間隙、`stream_size`；enco… | low | 🟡 部分 | encoder 先檢查 block 連續 `1a41191`；reader 仍只檢查不越界 |
| F-12 | `scalar_from_bytes` 是公開 API 卻不驗證 `data.len() == size`（size 1 + 空切片 p… | nit | ✅ 已修 | `1c7023e` |
| F-13 | 文件與程式碼脫節：`stream_size` 定義、「no-op roundtrip 位元組一致」的三種互相矛盾的說法、不存在的 `do… | nit | 🟡 部分 | `stream_size` 說明 `7426d4b`；其他矛盾說法未改 |
| F-14 | Dead code 被 `allow(dead_code)` 遮住：`scalar_as_i128` 在所有 feature 組合下都沒… | nit | ✅ 已修 | `423d0a1` |
| G-01 | `crimson_save_list_all_items` 與 `crimson_save_list_field_positions`… | high | ✅ 已修 | `d0c4ded` |
| G-02 | 改長度編輯 commit 後的「re-parse」不是驗證：`decode_blocks` 不會失敗，`Body::parse` 只讀… | medium | ✅ 已修 | `714bb66` |
| G-03 | `set_object_list_present(1)` 不檢查 list 是否已 present：對已有 N 個元素的 list 呼叫… | medium | ✅ 已修 | `d0c4ded` |
| G-04 | Deferred batch 期間的改長度編輯不遞增 `mutation_version`，讀取 API 卻已看到新的樹；`save-m… | medium | ✅ 已修 | `d0c4ded` |
| G-05 | Mutation 途中 panic 沒有 rollback：`h.blocks` 與 `h.save.body` 失去一致；之後的 in… | low | ✅ 已修 | `1a41191` |
| G-06 | 改長度的 list 操作一律以 `elements.len()` 覆寫 `count`，不檢查原本是否 `count == elemen… | medium | ✅ 已修 | `d0c4ded` |
| G-07 | 測試涵蓋：version 遞增測試只測 path setter；deferred 測試只測 clone 加 scalar；沒有並行、pa… | medium | 🟡 部分 | G-03／G-06 與版本序列測試 `d0c4ded`；多執行緒測試未加 |
| G-08 | Deferred batch 內，剛被 toggle 成 present 的 scalar 或 template 建出的元素，其 `st… | low | ✅ 已修 | `9e73dd9` |
| G-09 | List presence toggle 一律設 `absent_marker = !make_present`，不看原本的 absen… | low | 🟡 部分 | absent→absent 改為 no-op `d0c4ded`；present(1)→present(0) 是否多寫 1 byte 仍待實測 |
| G-10 | Element template 的缺口：只比 kind 與 size、不比 `type_name`；接受 placeholder 元素… | low | 🟡 部分 | placeholder 匯出拒絕 `714bb66`；scalar type name 需改 CRET 格式 |
| G-11 | `list_insert_element`、transplant、template insert 都不檢查插入元素的 class 是否屬… | low | ✅ 已修 | `714bb66` |
| G-12 | `crimson_save_write_to_file` 用 `std::fs::write` 直接覆寫目標，沒有暫存檔加 rename… | low | ✅ 已修 | `9e73dd9` |
| G-13 | `repr(C)` 結構的 layout 守門不完整：`CrimsonInventoryItemRecord`、`CrimsonChar… | low | ✅ 已修 | `9e73dd9` |
| G-14 | 四個列舉 API 以 class / 欄位名稱、寫死的 1.07 `PLAYABLE_CHARACTER_KEYS` 與 40 B `T… | low | ⬜ 未修 |  |
| G-15 | Transplant 不檢查 `target_handle != source_handle`，同一 handle 傳兩次即為 alia… | nit | ✅ 已修 | `9e73dd9` |
| G-16 | 文件與程式碼脫節：`c-abi-surface.md` 只列 47 個 `crimson_save_*` 符號中的 12 個；defer… | nit | 🟡 部分 | 符號目錄 `e0a9689`；個別函式文件的矛盾未改 |
| H-01 | `inspect_legacy_patches` 在 2.06 表上對每個 patch 都回 `None`：不收 header，`str… | medium | ✅ 已修 | `1900073` |
| H-02 | `serialize_iteminfo` / `write_iteminfo_to_file` 只輸出 body，沒有任何 Python… | medium | ✅ 已修 | `1900073`（新增 `serialize_iteminfo_with_header`） |
| H-03 | `serialize_pamt` / `write_pamt_file` 沿用 dict 裡的舊 header checksum：編輯後… | low | ✅ 已修 | `1900073` |
| H-04 | PAMT dict 往返以「目錄清單 × 各自檔案」攤平重建 `raw_files`，不保留原順序與孤兒檔；`path` / `name… | low | ✅ 已修 | `7fce1cb` |
| H-05 | PAPGT dict 往返從 entries 重建名稱緩衝區：offset 重疊時名稱互相覆寫、未引用的位元組消失、超大 offset… | low | ✅ 已修 | `1a41191` |
| H-06 | `pack_mod` / `create_pack_group*` 不拒絕已在 PAPGT 裡的 `group_name`：upsert… | medium | ✅ 已修 | `11fc8bd` |
| H-07 | `create_pack_group*` 與 `validate_game_dir` 就地改寫 live `meta/0.papgt`：… | medium | ✅ 已修 | `11fc8bd` |
| H-08 | 打包中途失敗會留下半成品群組目錄，之後重跑一律 `FileExistsError` | low | ✅ 已修 | `11fc8bd` |
| H-09 | `group_name` / `output_dir` 沒有驗證：`..`、絕對路徑可寫到預期目錄之外；`PackGroupBuilde… | low | ✅ 已修 | `11fc8bd` |
| H-10 | `[f32; 3]`、`[u32; 2]`、`[u32; 4]` 的 `WritePyValue` 不檢查長度，長度錯的 list 會寫… | low | ✅ 已修 | `1900073` |
| H-11 | Python 寫回路徑和 Rust `BinaryWrite` 不是同一份實作，而且完全沒有自動化測試；README 與 `script… | nit | ⬜ 未修 |  |
| H-12 | 條件欄位（`unk_pre_gimmick_visual`）寫回時不重算條件：只改 `equip_type_info` / `item_… | low | ✅ 已修 | `1a41191`（寫回後逐列重讀，讀不回來就報錯；不自動重算條件） |
| H-13 | `.pyi` 與實際 `#[pyfunction]` 脫節：缺 6 個函式、`ItemInfo` TypedDict 有 7 個已不存在… | low | ✅ 已修 | `8111abb` |
| H-14 | `docs/api.md` 等文件與程式脫節：`Language` 表全錯、`Crypto.ICE/AES` 宣稱可用但會被拒、「105… | low | ✅ 已修 | `8111abb` |
| H-15 | 沒有任何版本自檢：沒有 `__version__` 或 build stamp（版本永遠 `0.1.0`），Python 沒有 pave… | low | ✅ 已修 | `8111abb` |
| H-16 | 2.06 的 lossy 掃描錨點變弱且不收 header；`parse_iteminfo_tracked` / `_lossy` 在… | low | 🟡 部分 | `.pyi` 註明 2.06 名稱為空、錨點較弱 `3f1a124`；lossy／tracked 仍不收 header |
| H-17 | Python 可達的 abort／`PanicException` 面：`decompress_data` 的 `uncompresse… | low | ✅ 已修 | `0cdfd25` |
| H-18 | Feature gate 與死碼：只開 `python` 的組合 CI 從不編譯；Python 綁定沒有任何測試目標；`[u8; 22]… | nit | 🟡 部分 | 死碼 `PrefabData` 等已刪 `423d0a1`；CI 建只開 `python` 的組合 `eacff68`；`[u8; 22]`／`[u8; 9]` 與 Python 測試目標未處理 |
| H-19 | `serialize_skillinfo` 缺 `row_indices` key 時靜默寫出 pre-2.06 佈局；`index_o… | nit | ✅ 已修 | `1900073` |
| I-01 | CI 的 `cargo test --lib` 不帶 `c_abi`：約 397 個可執行測試中有約 286 個（所有 `src/c_a… | medium | ✅ 已修 | `45daf6f` |
| I-02 | live 測試把「install 在、但讀不出來」當成「沒有 install」：`gamedata_layout::extract_bi… | medium | ✅ 已修 | 共用 helper `d541ce2`、各模組 28 個 helper `cc45659` |
| I-03 | `src/lib.rs` 的測試混用寫死的 `GAME_DIR` 與 `CRIMSON_GAME_ROOT`：設了覆寫時，skill 檔… | low | ✅ 已修 | `b2a22f0` |
| I-04 | `test_skill_roundtrip_cross_version` 要嘛永遠 skip，要嘛在 `expect("dir not… | low | ✅ 已修 | `02395c6`；資料改放 private repo `5ad3a14` |
| I-05 | 存檔 encoder 的兩個 roundtrip 測試只讀 `%LOCALAPPDATA%` 的 live 存檔，不讀 `crimson… | low | ✅ 已修 | `d541ce2` |
| I-06 | live 存檔測試釘死 `slot107`（20+ 處複製貼上），驗的是那個 slot 碰巧是哪一版，不一定是最新版 | low | ✅ 已修 | `014a6c5`（改用 mtime 最新的存檔；`#[ignore]` probe 不動） |
| I-07 | 過時的版本註解與標籤：`lib.rs:117`「parser targets 1.05」、`:151`「1.04 baseline」、`… | nit | ✅ 已修 | `b2a22f0`、`7426d4b`、`423d0a1` |
| I-08 | `test_full_roundtrip` 讀 `out\iteminfo.pabgb`：內容是最後一次 `export_for_ce.… | low | ✅ 已修 | `374b8f5` |
| I-09 | CI 從不建置真正出貨的產物：只跑 Ubuntu，不建 `--features c_abi` 單獨組合、不建 Windows MSVC、… | low | ✅ 已修 | `eacff68`（Windows MSVC release dll job，`dumpbin` 檢查只依賴系統 DLL；`c_abi`、`python` 各自單獨建置） |
| I-10 | `.cargo/config.toml` 的註解過時：引用已刪除的 `build.yml` 與 windows-gnu wheel；現在… | low | ✅ 已修 | `7426d4b` |
| I-11 | CI 環境沒有釘選：CLAUDE.md 說「CI pins 3.12」但 workflow 沒有 setup-python；Rust t… | nit | 🟡 部分 | CLAUDE.md 說法修正 `7426d4b`；CI 釘 Python 3.12、`permissions: contents: read` `eacff68`；toolchain 刻意跟 stable、actions 未釘 SHA、dev→main 仍跑兩次 |
| I-12 | 模組層級的 `allow(dead_code)` 讓「全 feature 嚴格 clippy 抓得到真正的死碼」（`lib.rs:1-1… | low | ✅ 已修 | `423d0a1` |
| I-13 | `claude_md_files_stay_under_40_kb` 量的是工作樹位元組：Windows（CRLF）與 CI（LF）的上… | nit | ✅ 已修 | `186c7cc` |
| I-14 | `dump_gamedata_keys.py`：`--version` 預設 `1.08`，照 runbook 不帶參數跑會覆寫已提交的… | low | ✅ 已修 | `186c7cc` |
| I-15 | key snapshot 不涵蓋 bridge 實際讀的 `faction`、`stringinfo`，也不涵蓋任何 `*.dlc_01… | low | ✅ 已修 | `631f693`（加 `faction`、`stringinfo` 與每張 DLC 表，65 檔） |
| I-16 | `extract_gamedata_bin.py` 沒有覆寫保護：版本封存是不可重建的 ground truth，但 `--versio… | medium | ✅ 已修 | `186c7cc` |
| I-17 | `update_ce_table.py` 的 `props_from_bin` 少了空 `inventory_info_list` 的防… | low | ✅ 已修 | 空清單防護 `374b8f5`、docstring 範例與 `read_paver` 讀 `CRIMSON_GAME_ROOT` `a0a20ed` |
| I-18 | 腳本各自維護 install 候選清單、不讀 `CRIMSON_GAME_ROOT`；多個腳本寫死個人路徑；`scripts/READM… | nit | ⬜ 未修 |  |
| I-19 | `setup-fork-guards.ps1`：原生指令失敗不會停（`$ErrorActionPreference` 管不到）；hook… | low | 🟡 部分 | 原生指令失敗即停、hook 未裝時明說 `374b8f5`；`gh` API 與上游改名未擋 |
| I-20 | `tools/git-crypt.exe` 的來源、版本、雜湊與授權沒有任何文件 | nit | ⬜ 未修 |  |
| I-21 | 依賴：沒有 `cargo audit` / `deny.toml` / CI 稽核步驟；`Cargo.lock` 有兩代 RustCry… | nit | ⬜ 未修 |  |
| I-22 | `character_info.rs` 的測試其實是 37 個與 characterinfo 無關的調查 probe（計畫書寫 49 個… | low | ⬜ 未修 |  |
| I-23 | `references/store_info.hexpat` 沒有版本標記，描述的是 2.04 之前的列頭，`BString` 定義與專… | nit | ⬜ 未修 |  |
| NEW-1 | 2.06 skill buff 的 inline condition 沒讀，78% 退成 raw fallback | medium | ✅ 已修 | `475b3fa` |
| E 新發現 | `DyeExtraLayer::flag` 註解「0/1 observed」過時（2.06 為 1/2/19） | nit | ✅ 已修 | `b90dfcf` |
| F 新發現 | deferred batch 內 presence toggle 後 scalar 寫入用 stale start/end | low | ↪ 併入 | → G-08 |
| D 新發現 | stage `_with_header` 多出 2 個雜訊 key | low | ✅ 已修 | `a6857ac`（隨 D-01） |
| D 新發現 | `mission_info` 模組文件把漏掉的列說成只有 `Challenge_*` | nit | ✅ 已修 | `a6857ac` |
| I 新發現 | `find_characterinfo_header`／`find_questinfo_header` 用 `unwrap_or_default()` | low | ✅ 已修 | `cc45659` |

149 條 finding：↪ 併入 2、✅ 已修 100、➖ 不修 2、⬜ 未修 24、🟡 部分 21。另有上表最後 6 列的新發現。

<!-- status-table:end -->

---

## 本機實測（主 session）

| 項目 | 結果 | 影響的 finding |
|---|---|---|
| 全套測試 `cargo test --lib --features c_abi -- --test-threads=4` | **399 passed, 0 failed, 48 ignored**（242 s；live 2.06 install 與 fixtures 都在） | 基準 |
| ItemInfo live byte roundtrip（暫時 probe） | base **7,068 / 7,068** 列、`dlc_01` **8,218 / 8,218** 列逐列位元組一致；`dlc_01` 的 row index 沒有一列等於自身位置（0 / 8,218） | C-02（缺測試成立，資料面無問題）、C-10（真實資料不出現） |
| `to_legacy_layout(2.06)` 對照 `2.05.01`（135 張共有表） | 135 張全部辨識為 2.06 pair；用重建後的寬度讀 2.05.01 header 全部吻合；2.05.01 header 全部依 offset 排序；共有 key 的列 310,597，其中 208,505 列位元組一致；10 張表的共有 key 相對順序不同 | B 審計重點 3（見下方 B-1 補充） |
| 2.06 header 寬度歧義（15 種組合，`parse_named_header` + `rows_are_packed`） | base 135 張：134 張剛好 1 種，`autospawnfilterinfo` 2 種（(1,2)、(2,1)）；`dlc_01` 有 64 張只有 header、沒有 body，無法用 packing 排除，各有 5 種組合通過 | B-04、B-08 |
| D-01 `_with_header` loader 實際 key 集合 vs header | missioninfo 缺 **2,444**；questinfo 缺 1（key 1）；stageinfo 缺 **3,198**、另有 2 個 header 沒有的雜訊 key、37 個重複列；gimmickinfo 缺 **140**；knowledge、questgauge、sublevel、character 0 | D-01 |
| 2.06 body 不給 header 時 body-only 掃描器的列數 | mission、quest、knowledge、gimmick、questgauge、sublevel 為 0；**stage 39、character 223**（全是雜訊或錯位列） | B-02 第 5 點 |
| D 區塊沒有 merge API 的表，其 `dlc_01` 副本裡 base 沒有的 key | mission 665、quest 53、stage 1,490、gimmick 1,378、questgauge 14、sublevel 1 | D 報告「未能驗證」第 4 點（成立） |
| D-02 characterinfo 雜湊 key 的 lo24 碰撞 | 359 個 hi8 ≠ 0 的 key，lo24 落在另一列 key 上的有 **0** 個 | D-02：「回傳別的角色名稱」在 2.06 不會發生，只會 `NOT_FOUND` |
| 存檔 ChaCha20 初始計數器（43 份存檔，header `0x1A..0x1E`） | 計數器呈均勻隨機（最大 `0xFACA2F67`）；0 份會跨過 2³²；1.8 MB 存檔每次寫入的跨越機率約 6.5×10⁻⁶ | F-06（機制成立，機率極低） |
| no-op 重編 `Body::write(decode_blocks(raw)) == raw`（fixtures + live save，43 份） | 40 份一致；**2 份不一致**，兩份都是編輯器寫出的 fixture（`1.10/broken_save_after_length_change/use_editor_add_sugar`、`1.10/sealed-artifact-challenge/editor-edit/after`）；1 份非存檔（`initialize_input.save`） | F-04、F-08、F-13（見 F 節） |
| B-1 差異來源：不一致的共有列，第一個不同位元組落在哪裡 | 落在重建出的列頭（`[u32 len][name][key]`）8,570 列，全部集中在 4 張表（multichangeinfo 8,307、itemuseinfo 257、gimmickgroupinfo 4、conditioninfo 2）；落在列頭之後 92,858 列；只有長度不同 664 列。如果重建有錯，每張表的每一列都會在列頭出錯；其他 131 張表的列頭全部一致，208,505 列整列一致，所以這 4 張表是名稱內容真的改了，不是重建錯誤 | B 審計重點 3（**重建正確**） |
| F：2 份 no-op 重編不一致的 fixture | 兩份都只差 `FactionSaveData` 裡的一個 u32 絕對 offset：目前的 encoder 寫出「欄位之後那一個位元組」，檔案裡的值少 241（`use_editor_add_sugar`）或 64（`editor-edit/after`）。這兩份正是舊版編輯器寫出、用來釘住 `_factionNodeApplySkillList` relocation bug 的檔案（`docs/save-loader-length-change-crash.md`、private repo README），所以目前 encoder 會把 stale offset 修好是預期行為；同組的 `editor-edit/before` 一致 | F-04、F-08、F-13：不是新缺陷 |
| `cargo check --lib --features python`（不含 `c_abi`）與同組合的 `clippy -D warnings` | 都通過，沒有 warning；`--features c_abi` 單獨 check 也通過 | H-18（只開 `python` 能編譯；「CI 從不編譯這個組合」仍成立） |
| `cargo build --release --features c_abi` 的 DLL 依賴（`dumpbin /dependents`） | 1,350,144 B；只依賴 `KERNEL32.dll`、`ntdll.dll`、`bcryptprimitives.dll`、`api-ms-win-core-synch-l1-2-0.dll`，**沒有 VC runtime／UCRT**，CRT 靜態連結有效 | I-09（零 transitive 依賴 confirmed；CI 不建這個產物仍成立） |
| skill raw fallback（live 2.06，Rust） | `test_skill_roundtrip_1_05` 印出 `2163 entries (raw fallback=1683)`，測試通過 | C-03、**NEW-1** |
| F-01 位元組層級重現（`1.10/save.save`，block 7 `InventoryItemContentsSaveData` 的一個 absence marker 0x01→0x02） | 重新 decode 得 809 B undecoded、2 個 `Unknown`；`Body::write` 回 **Ok**，輸出比原 body **少 809 B**，沒有任何錯誤 | **F-01 confirmed**（C ABI 的每個改長度操作與 deferred `end_*` 都走 `Body::write`） |
| 全套測試以預設執行緒數（本機 32）跑一次 | 399 passed、56 ignored（48 + 8 個暫時 probe），行程沒有死；沒有量 commit charge 峰值 | C-01：一次通過不足以證明 `--test-threads=4` 可拿掉 |
| A-16 2.06 英文 PALOC（45 檔、222,887 項） | 檔內重複 key 0、跨檔同 key 不同值 0、非 UTF-8 0 | A-16（真實資料不觸發） |
| `cargo audit`／`cargo deny` | 本機沒有安裝，未執行 | I-21 仍 NEEDS-RUN |
| `test_skill_roundtrip_cross_version` 的 `G:` 路徑 | `G:\我的雲端硬碟\temp\Crimson Desert` 只有 `bin_backup`，測試永遠 skip；若放入 1.0x install，`bin_dir()` 回傳 2.01+ 路徑，會在 `expect("dir not found")` panic | I-04（confirmed） |

---

## A. 封存容器與加密

查證 agent 另外讀了依賴原始碼（`lz4_flex 0.11.6`、`chacha20 0.10.0`、`cipher 0.5.1`），並對 74 個 live `0.pamt`（base + `dlc_01`，2,199,679 個檔案項）做唯讀掃描：compression 只有 0/1/2（**沒有 zlib**），crypto 只有 0/3（ChaCha20 共 119,213 檔、504 MB），沒有未知 code、沒有 u32 溢位、沒有 0 位元組項；ChaCha20 計數器最接近上限的檔案仍有 38,720 個 block 的餘裕；74 個 PAMT 全是 2.04+ 的 relative-parent trie。

| ID | 判定 | 嚴重度 | 查證重點 |
|---|---|---|---|
| A-01 | CONFIRMED | medium | `trie.rs:130` 在 `:133` 寫 cache 之前遞迴；`AbsoluteParent` parent == offset 會無限遞迴；checksum 可重算。 |
| A-02 | CONFIRMED | medium | `pamt.rs:248`、`:257` 以未檢查的 u32 count 配置；`lz4_flex` 的 `decompress_safe.rs:355` 會先 `vec![0; min_uncompressed_size]`。 |
| A-03 | CONFIRMED | medium（live 無 zlib，屬潛在） | zlib `read_to_end` 無上限；LZ4 輸出過短時 `truncate` 後回 Ok。 |
| A-04 | CONFIRMED | **改 low** | 只影響 Python builder 使用非預設 `crypto=3` 且 `encrypt_info` XOR ≠ 109 的情形。 |
| A-05 | CONFIRMED | low | live 沒有這些 code。 |
| A-06 | PARTLY | **改 nit/low** | panic 真實（`apply_keystream` 的 `.expect`）；但 `seek` 不會 panic，「數十 GB」不成立（加密總量 504 MB）。補充：檔名雜湊剛好 `0xFFFFFFFF` 的非空檔必定 panic。 |
| A-07 | **REFUTED** | 刪除或降為防禦性 nit | 現行 std 合約允許 count 為 0 時指標為 null（只要求對齊）；live 也沒有 0 位元組項。 |
| A-08 | CONFIRMED | low | release wrap 後 `read_exact` 失敗 → `BODY_PARSE`，不會越界讀。 |
| A-09 | CONFIRMED | low | release 下只有 \|k\| ≤ offset 的負值會 panic；`-1` 會。 |
| A-10 | CONFIRMED | low | 四種不對稱都在。 |
| A-11 | CONFIRMED | low | |
| A-12 | CONFIRMED | low | 來源已改為 `ad7596e`。補正：2³² 次迴圈不依賴 A-02，「被 A-02 蓋過」只對 4 GB `Vec` 成立。 |
| A-13 | CONFIRMED | low | 版本閘門只存在於文件與 live 測試。 |
| A-14 | CONFIRMED | low | 與 I-01 同根因。 |
| A-15 | CONFIRMED | low | |
| A-16 | CONFIRMED（程式碼） | **改 nit** | 主 session 實測 2.06 英文 PALOC（45 檔、222,887 項）：檔內重複 key 0、跨檔同 key 不同值 0、非 UTF-8 0，真實資料不觸發。其他語言未查。 |
| A-17 | CONFIRMED | nit | |
| A-18 | PARTLY | nit | 第 6 項（`hashlittle2` 命名）不算錯：`*pb = 0` 時 hashlittle2 回傳的正是這個 `c`。其餘成立。 |
| A-19 | CONFIRMED | nit | |

新發現：無獨立的新缺陷（`0xFFFFFFFF` 計數器併入 A-06）。

---

## B. Static-info 表頭／列頭佈局層

查證 agent 以 Python 重寫 `parse_named_header`、`rows_are_packed` 與 mission／gimmick 掃描器，對 2.06 封存（base + `dlc_01`，211 張有 body 的表、550,773 列）唯讀執行。

| ID | 判定 | 嚴重度 | 查證重點 |
|---|---|---|---|
| B-01 | PARTLY | **改 low**（或併入 B-02） | fallback 屬實；`legacy_pair(` 正式碼呼叫是 23 個檔案 24 處（不是 22）；「辨識失敗回空 `Vec`」在 2.06 之前就是 lossy parser 的行為，`legacy_pair` 只是沿用；案例 5 與案例 1 相同。 |
| B-02 | CONFIRMED | medium | 第 5 點（雜訊列）實測成立：2.06 body 不給 header 時，stage 回 39 列、character 回 223 列（mission、gimmick 等為 0）。 |
| B-03 | CONFIRMED | low | |
| B-04 | PARTLY | low（可 nit） | 測試缺口屬實；「`WIDTHS` 缺 (1,8)、(1,12)、(4,12)」不算缺陷：base 表只用到 `WIDTHS` 裡的組合。本機實測：base 135 張中 134 張只有 1 種組合通過；`dlc_01` 有 64 張只有 header，5 種 key 寬度都通過（無 body 可排除）。 |
| B-05 | CONFIRMED | low | |
| B-06 | PARTLY | low（可 nit） | stringinfo 也有比對 header／body hash，不是只有 iteminfo；2.06 上 header key 與 body key 0 處不一致。 |
| B-07 | PARTLY | **改 nit** | `export_for_ce._read_header_rows` 依設計只讀 iteminfo；Python 讀不了的表回 None，不是錯 key，且都不在 `PABGH_TABLES`。 |
| B-08 | PARTLY | low | 「沒有真實表觸發」不成立：2.06 `autospawnfilterinfo`（1 列）同時通過 (2,1) 與 (1,2)；`WIDTHS` 順序選到正確的 (2,1)，且沒有 bridge 讀這張表。 |
| B-09 | PARTLY | nit | panic 真實，但報告的重現輸入錯誤（長度檢查先回 None）；正確重現：`from_legacy_layout(&[], &[0u8; 9], 9, 4, &[])`。 |
| B-10 | CONFIRMED | nit | |
| B-11 | PARTLY | nit | 第 2、3 點同樣適用於手寫 bridge；第 4 點來源有誤（2.06 之前 `read_row_head` 也只檢查 UTF-8）；2.06 無名稱含 NUL。 |
| B-12 | PARTLY | nit | 第 5 點後果有誤：32-bit 上是 capacity-overflow panic（可被 `catch_unwind` 接住），不是 abort；32-bit 也不是建置目標。 |

**審計重點 3 補充（`legacy_pair` vs 2.05.01，本機實測）：** 重建出的 legacy header 寬度與 2.05.01 全部吻合，2.05.01 header 一律依 offset 排序，所以「重建後 header 同序」成立。共有 key 的列有 67% 位元組一致。不一致的列，第一個不同位元組幾乎都在列頭之後（遊戲資料本身的改動，例如 2.06 inline 的 condition）；落在列頭的只集中在 4 張改了名稱的表（見「本機實測」的 B-1 差異來源）。結論：`legacy_pair` 的重建與真正的 2.05.01 佈局一致。

新發現：無（`autospawnfilterinfo` 的歧義併入 B-08）。

---

## C. 核心大型解析器：ItemInfo / Skill / StringInfo

查證 agent 讀了 `a9d7012` 的 diff，並用 venv 的 `crimson_rs` 唯讀跑了 live install 與 `X:` 封存。

| ID | 判定 | 嚴重度 | 查證重點 |
|---|---|---|---|
| C-01 | CONFIRMED | low | `a9d7012` 已把未檢查的 u32 → `with_capacity` 換成 `read_count`；`TailSizeCache` 是每次 parse 的區域變數，只 `get`／`insert`，不會造成非決定性。CLAUDE.md:37、:69 與 `scripts/CLAUDE.md:72`、:80 的說法過時。「`--test-threads=4` 是否仍必要」未實測（NEEDS-RUN）。 |
| C-02 | CONFIRMED | medium | 沒有任何測試做 live iteminfo 的位元組往返。主 session 的 Rust probe 實測 base 7,068 列與 `dlc_01` 8,218 列逐列一致，所以缺的只是測試。 |
| C-03 | PARTLY | **改 medium** | 程式碼部分成立；「文件沒記錄基準」錯（`docs/api.md:296` 有約 10% 的舊數字，但沒有測試釘住）。後果已經在 2.06 發生，見下方 NEW-1。 |
| C-04 | CONFIRMED | low | 真實資料超過上限屬假設。 |
| C-05 | CONFIRMED（實測） | **改 nit** | 10 位元組 header 實測 panic（`mod.rs:571`），C ABI 會轉成 `PANIC`；只有壞輸入才觸發。 |
| C-06 | CONFIRMED | low | `load_from_bytes` 的文件還說它是「preferred」。 |
| C-07 | PARTLY | low | 「有 header 的路徑以列長度為界」偏弱：列長度來自 header 的 `size`，構造的 header 可以設成整個 body；`size_of` 數字是估計值。 |
| C-08 | CONFIRMED（實測） | low | 刪掉最後一列 header 並把 count 減 1，得到 7,067 個物品、沒有錯誤。 |
| C-09 | CONFIRMED（實測） | low | 改了 skill header 第 0 列的 key，parse + serialize 成功，輸出 header 等於**原本的** header。根因是 B 區塊共用的 `to_legacy_layout`，不只 skill。 |
| C-10 | CONFIRMED | low | live 2.06 base 與 DLC 的旗標只有 0／1，沒有「01 + count 0」，真實資料不出現。 |
| C-11 | CONFIRMED | **改 nit** | 需要第一列 hash 剛好為 0（2⁻³² 機率）。 |
| C-12 | CONFIRMED | nit | 反向 map 來自 `6c32611`（2026-05-16）。 |
| C-13 | PARTLY | low | 「19 個 getter 完全沒有文件」誇大：`lookup_inventory_info`、`lookup_max_endurance` 有被提到。`:172-174` 的「72 bytes」註解與 const assert 的 80 bytes 矛盾。 |
| C-14 | CONFIRMED | low | |
| C-15 | CONFIRMED | nit | |
| C-16 | CONFIRMED | nit | |
| C-17 | CONFIRMED | nit | string_info 的 allow 來自 `e335870`（2026-05-13），不是 `06117ef`／`56a57da`。 |
| C-18 | CONFIRMED | low | 與 I-04 同一件事；也讓 `a9d7012` commit 訊息裡「cross-version roundtrips unchanged」的證據落空。 |
| C-19 | CONFIRMED（程式碼） | nit | live 2.06 iteminfo 52,288 + 62,194 個 `CString` 沒有非 UTF-8。 |

### NEW-1 — 2.06 的 skill buff 佈局漂移沒有被偵測到（medium，新發現）

- **現象：** 2.06 的 skill 有 **1,683 / 2,163（78%）** 個 entry 靜默退成 raw fallback。主 session 以 Rust 確認：`test_skill_roundtrip_1_05` 在 live 2.06 印出 `2163 entries (raw fallback=1683)`，而且測試通過。查證 agent 用 Python binding 對封存量測：2.04 303／2,159、2.04.08 303／2,159、2.05.01 304／2,160（都是 14%）。
- **哪些列失敗：** 2.06 列重建成 2.05.01 佈局後，和 2.05.01 位元組一致的 480 列全部解得開；有變動的 1,683 列全部 fallback。1,680 個有變動的共有列中，有 1,347 列的長度變化剛好是「−3 bytes × 非 null buff 數」，指向 `BuffData` 每個 buff 少了 3 bytes，`read_common_base` 沒有建模。
- **為什麼沒被抓到：** byte roundtrip 仍然通過（raw fallback 原樣保存位元組），`_probe_skill_entry_failures` 回報 2163/2163 ok，`validated-patches.md:35` 記為 clean。正是 C-03 擔心的情境。
- **影響：** Python 呼叫端 78% 的 skill 拿到 `buff_level_list = None`；C ABI 只用名稱，不受影響。
- **來源：** 遊戲資料在 2.06 改變，`95fa25f` 漏掉。
- **確信度：** fallback 數量 confirmed（Rust 與 Python 兩邊一致）；「每個 buff −3 bytes」是 plausible，需要對照 2.05.01 與 2.06 的 `BuffData` 走一次 tandem walk 才能定案。

新發現另一條（nit）：`a9d7012` 的 commit 訊息以「cross-version roundtrips unchanged」為證據，但那個測試只可能 skip（見 C-18）。

---

## D. 一般 gamedata key-resolver 橋接

查證 agent 以 Python 逐位元組模擬掃描器（`to_legacy_layout` 重建 + 相同規則與游標推進），重現 mission 4,428、knowledge 6,839 列；主 session 的 Rust probe 給出相同數字。

| ID | 判定 | 嚴重度 | 查證重點 |
|---|---|---|---|
| D-01 | CONFIRMED，**報告低估** | medium | 實際漏列：mission **2,444** / 6,872（key 為 `0xFFFFF3EE`–`0xFFFFFFFE`，其中 1,826 個 `Mission_*`、531 個 `Challenge_*`，不只是 Challenge）；stage **3,198** / 53,313（1,177 依 key 規則，2,021 個含逗號的 `LevelSequencerSpawn_UUID_…`），另多 2 個雜訊 key（1 `UI_NpcStore_Title`、256 `Platform_Sequencer_00`，`lookup_string_key` 會回 OK 加垃圾名稱）；gimmick **140**（12 依 key 規則、128 依名稱／路徑規則）；quest 1（`StartGame`）。knowledge、questgauge、sublevel 為 0。 |
| D-02 | PARTLY | **改 low**（建議） | 程式碼路徑如所述：359 列雜湊 key 透過 `lookup_string_key` 與 `resolve_portrait` 全部查不到。PALOC 實測：這 359 列的名稱放在 hi32 = 完整 key（`(full_key << 32) | 0x30`），lo24 沒有，所以 `lookup_display_name` 全回 `NOT_FOUND`。lo24 碰撞 0 個（agent 與主 session 各自算出），「回傳別的角色名稱」在 2.06 不會發生，嚴重度的主要論據不成立。 |
| D-03 | CONFIRMED | medium | 與 A-14、E-08、I-01 同根因。`main_quest_chapter.rs:869-872` 的「everything here runs on CI」不成立。 |
| D-04 | PARTLY | low（可 nit） | 不檢查 key 屬實；報告的重現錯：CharacterKey 1000000 是真實存在的列，加上 `by_key` 檢查仍回 OK。 |
| D-05 | PARTLY | low | 實測（空 header + 2.06 `dlc_01` body）：knowledge 新增 0 列；characterinfo 新增 **18 列雜訊**（例如 key 15 `_D`）。18 列都沒有和 287 個真正的 DLC key 衝突，所以之後正確的 merge 不會被擋。 |
| D-06 | CONFIRMED（健壯性觀察） | low（可 nit） | |
| D-07 | CONFIRMED | low | stage live 測試在少 3,198 列的情況下仍通過，正好印證。 |
| D-08 | PARTLY | nit | ≥ `0x4300` 的 key 是 12 個（不是 4），1.08 就有 8 個，註解從來沒成立過；目前的 key 仍全在 14 位數範圍內。 |
| D-09 | CONFIRMED | nit | 計數小誤：手寫 16 個加巨集會清 `out_key`，不是 22。 |
| D-10 | CONFIRMED | nit | 2.06 mission 4,428 個 key 全部唯一，兩邊不會分歧。 |
| D-11 | CONFIRMED | nit | 12 項抽查成立。 |

另外：D 報告「未能驗證」第 4 點成立。mission、quest、stage、gimmick、questgauge、sublevel 的 `dlc_01` 副本分別有 665、53、1,490、1,378、14、1 個 base 沒有的 key，而這些 bridge 沒有 DLC merge API。

新發現：stage 的 2 個雜訊 key（併入 D-01）；`src/mission_info/mod.rs:25-28` 把漏掉的列說成只有 `Challenge_*`（nit）。

---

## E. 染色／Prefab 子系統

查證 agent 讀了 2.06 封存的三張 dye 表做資料面驗證。

| ID | 判定 | 嚴重度 | 查證重點 |
|---|---|---|---|
| E-01 | CONFIRMED | medium | 1,703 列中只有 392 列的 `slot_count` 等於 mask 聯集的 popcount（714 列較大、597 列較小）；rustdoc、`c_abi/part_prefab_dye_slot_info.rs:188-189`、`dye-editor-scope.md:609` 與 v3 節都把它當成權威 slot 數，只有 `:342-344` 寫相反。 |
| E-02 | CONFIRMED | low | Python 移植 `record_starts` 重現報告輸入成立。 |
| E-03 | CONFIRMED | low | |
| E-04 | PARTLY | low | 三個表 loader 成立；`item_part_prefab` 在 iteminfo／stringinfo 失敗時會回 `BODY_PARSE`，引用的 `:282-291` 是 `key_by_name`，不是 loader。 |
| E-05 | CONFIRMED（程式碼）／plausible（後果） | low | |
| E-06 | CONFIRMED（程式碼） | **改 nit** | 2.06 沒有任何 `.pac` 路徑含大寫、反斜線、空白或非 ASCII。 |
| E-07 | CONFIRMED | **改 nit／併入 A-01、A-02** | 不是新的攻擊面，`crimson_paz_extract_file` 早已解析呼叫端給的 PAMT。 |
| E-08 | PARTLY | low（與 A-14、I-01 同根因） | 計數有誤：`*_null_args` 7 個、`*_empty_bytes_*` 4 個；報告建議的 `assert!(false)` 會先被 clippy 擋下。 |
| E-09 | CONFIRMED | nit | |
| E-10 | CONFIRMED（程式碼） | nit | 2.06 的 1,703 個 key 全部唯一。 |
| E-11 | CONFIRMED | nit | |
| E-12 | CONFIRMED（程式碼） | nit | 2.06 十個色票沒有重複 RGB。 |
| E-13 | PARTLY | nit | extra-layer flag 那一項錯：2.06 的值是 1（15）、2（2）、19（1），沒有 0，照「0/1」檢查會丟掉 3 列。其餘常數屬實。 |
| E-14 | PARTLY | nit | `..._lookup_slot_extra_layer_mask` 其實有在 `dye-editor-scope.md:262`。 |
| E-15 | CONFIRMED | nit | |
| E-16 | CONFIRMED | nit | |

新發現：`src/part_prefab_dye_slot_info/mod.rs:154` 說 `DyeExtraLayer::flag`「0/1 observed」，2.06 實際為 1/2/19（nit，文件過時；修 E-13 時不能照這句加檢查）。

---

## F. 存檔格式核心

查證 agent 用 venv 的 `crimson_rs`（建置時間晚於 `src/save` 最後一次修改）唯讀處理了 18 份 fixture 與 12 份 live 存檔，構造輸入都在記憶體裡建立。主 session 的 no-op 重編 probe 與它的 offset 分析結論一致。

| ID | 判定 | 嚴重度 | 查證重點 |
|---|---|---|---|
| F-01 | CONFIRMED（含修正；主 session 位元組層級重現：改 1 byte → `Body::write` 回 Ok 並靜默少寫 809 B） | high（影響面）；目前所有已知存檔都不觸發 | 只有 C ABI 走得到：載入不檢查完整度，每個改長度的操作與每個 deferred `end_*` 都把**全部** block 交給 `Body::write`，`encode_body` 只檢查 block 數。Python 沒有任何 binding 呼叫 body encoder。前提很便宜：把 `1.10/save.save` block 7 的一個 absence marker 從 0x01 改成 0x02 仍能載入，產生 809 B undecoded 與 2 個 `Unknown`。後果：undecoded 位元組消失、placeholder 元素變大、非 inline locator 的指標被改寫。報告漏掉兩點：strict marker walk 接受非 inline child；`collect_decoded_ranges` 把非 inline child 的範圍算成已解，所以這種丟失**不會**出現在 `undecoded_ranges`。報告過度的部分：沒有 child 的 locator 會明確失敗，被跳過的 TOC entry 會被 block 數檢查擋下。30 份存檔全部 0 undecoded、0 `Unknown`、0 placeholder。 |
| F-02 | PARTLY | **改 low**（除非在固定 page file 的機器上重現 abort） | 這台機器（61.6 GiB RAM、系統管理 page file）上 128 GiB 配置**成功**，回傳 260,029 個從 data section 讀出的垃圾 entry。abort 與否取決於機器，報告漏了「垃圾 entry」這個後果。 |
| F-03 | CONFIRMED（實測） | medium | 自我參照的 `child_payload_offset` 讓 python.exe 在 0.48 s 內 SIGSEGV（exit 139）；對照組正常。指數時間未測。 |
| F-04 | CONFIRMED | medium | `src/save` 只有 14 個 `#[test]`；`test_faction_node_offset_relocates_through_trailing_pad` 用的 fixture 現在解出 0 B `trailing_pad`，真實資料上已經沒有測試走到 pad relocation 路徑。 |
| F-05 | CONFIRMED | low | 實測 `uncomp_size = 0xFFFFFFFF` 在這台機器回 ValueError，沒有 abort。 |
| F-06 | CONFIRMED（實測） | low | 計數器 `FF FF FF FF` 實測 `PanicException`；正確條件是 counter + ceil(len/64) > u32::MAX。主 session 量測：43 份存檔的計數器均勻隨機，每次寫入約 6.5×10⁻⁶ 機率。 |
| F-07 | CONFIRMED（實測） | low | (1) 實測 index out of bounds panic；(2) off-by-one 真實，但只有 legacy walk 走得到。 |
| F-08 | CONFIRMED | low | 30 份存檔全部 0 B `trailing_pad`，屬潛在；改壞 1 byte 就會讓 3 個 block 退成帶 1–3 B pad，所以漂移時很快會觸發。 |
| F-09 | CONFIRMED | low | 所有存檔的 list 都是 `zero1_count_u24`，空 list 的歧義屬潛在。 |
| F-10 | CONFIRMED | low | 零編輯也會重新封裝。 |
| F-11 | PARTLY | low | 30 份存檔都符合佈局假設；來源一處有誤：Python 的 `write_save_with_nonce` 不會呼叫 body encoder，encoder 的 `as u32` 從 Python 走不到。 |
| F-12 | PARTLY | nit | 不是公開 API（`mod save;` 是私有的），呼叫端都有檢查長度；但其中一個檢查可以被繞過（見 G-08）。 |
| F-13 | CONFIRMED | nit | `stream_size` 實測等於 body 長度（絕對結束 offset），不是 `toc.rs:10` 說的 data section 大小。no-op 重編：引擎寫出的存檔全部位元組一致，只有兩份編輯器 fixture 各有 1 個 stale offset（主 session probe：40 / 42 一致，不一致的正是這兩份），所以 `lib.rs:1064-1073`、`:1216-1222` 的註解過時。 |
| F-14 | CONFIRMED | nit | save 模組在 `lib.rs:80`（不是 :78）。 |

新發現：查證 agent 提出的「deferred batch 內 presence toggle 之後 scalar 寫入用 stale `start`／`end` 檢查」與 **G-08 是同一個缺陷**（G-08 已 CONFIRMED），補充細節併入 G-08：正確長度的寫入會被 `LENGTH_MISMATCH` 拒絕，0 長度寫入反而被接受（1 byte 型別 panic、2/4/8 byte 變 0、其他變成空 `Bytes`，release 下 encoder 只靠 `debug_assert`）。

---

## G. 存檔編輯 C ABI

查證 agent 另外唯讀查看了 C# 宿主（`D:\Github\CrimsonAtomtic`）以判斷情境是否實際可達。

| ID | 判定 | 嚴重度 | 查證重點 |
|---|---|---|---|
| G-01 | CONFIRMED | high | `all_items.rs:719-721`、`positions.rs:528-530` 沒有取 `save_ffi_lock()`。C# 端實際會在 `Task.Run` 上呼叫 `ListAllItems`／`ListFieldPositions`，也在 `Task.Run` 上做批次修改；`save-mutation-version.md:124-125` 的「single-threaded by contract」與鎖本身的說明互相矛盾。 |
| G-02 | CONFIRMED | **改 medium** | commit 後的 re-parse 不會失敗，transplant 文件承諾的 `MUTATION_INVALID` + rollback 不成立；但 C# 宿主沒有呼叫 raw `TransplantListElement`／`ListInsertElement` 的正式路徑。第 3 點（`filter_map` 丟 block）實務上不可達。 |
| G-03 | CONFIRMED | medium | |
| G-04 | CONFIRMED | medium | 直接違反 `save-mutation-version.md:110-115`、`:157-161`。 |
| G-05 | PARTLY | **改 low** | 缺陷真實，但沒有證據顯示真實存檔在 commit decode 期間會 panic。 |
| G-06 | CONFIRMED | medium | |
| G-07 | CONFIRMED | medium | 37 個載入存檔的測試都經 `find_save()`（只用 slot107），沒有一個用 `test_fixtures`。 |
| G-08 | CONFIRMED | low | C# 染色編輯器同一批次內不會對同一欄位既 Set 又 SetPresent，所以目前不觸發。F 區塊查證另外指出：toggle 之後正確長度的寫入會被 `LENGTH_MISMATCH` 拒絕，0 長度寫入反而被接受（1 byte 型別 panic、2/4/8 byte 變 0、其他變空 `Bytes`，release 下 encoder 只靠 `debug_assert`）。 |
| G-09 | NEEDS-RUN | low | 程式碼部分成立；是否真的多寫 1 byte 需在 legacy-walk block 上實測。 |
| G-10 | CONFIRMED | low | |
| G-11 | CONFIRMED | low | |
| G-12 | PARTLY | low（或 nit） | C# 端 `SaveBackupService` 每次存檔前會先備份，「毀掉唯一存檔」過度。 |
| G-13 | CONFIRMED | low | |
| G-14 | CONFIRMED | low | |
| G-15 | CONFIRMED | nit | |
| G-16 | CONFIRMED | nit | `crimson_save_*` 共 47 個符號，`c-abi-surface.md` 只列 8 個。 |

新發現：無（`decode_one_list_element_bytes` 的 off-by-one 屬於 F-07）。

---

## H. Python 綁定與 mod 打包

查證 agent 用 venv 的 `crimson_rs` 做了只讀 live install 的記憶體內重現（沒有呼叫 `pack_mod`、`create_pack*`、`validate_game_dir`）。結果：16 confirmed、3 partly、0 refuted；原報告四個 plausible 被資料定案。

| ID | 判定 | 嚴重度 | 查證重點 |
|---|---|---|---|
| H-01 | CONFIRMED | medium | 實測：以真實名稱查回 None。子項「offset 位移 4+len+4」應為 `len(name)`。 |
| H-02 | PARTLY | medium | 行為實測成立（改長度後用舊 header 重 parse 失敗；`string_key` 編輯被丟掉）；來源有誤：舊 `.pabgh` 在改長度時同樣會過時，只有「丟掉 `string_key` 編輯」是 95fa25f 新引入。 |
| H-03 | CONFIRMED | low | 實測：改 `chunks[0].size` 後 `parse_pamt_bytes` 報 checksum mismatch。 |
| H-04 | CONFIRMED | low（可 nit） | 35 個 live PAMT 的範圍都連續、無孤兒檔，純往返安全。「parse 不檢查目錄字串」錯：`pamt.rs:296-309` 有驗。 |
| H-05 | CONFIRMED | low | live `0.papgt` 39 項、名稱緩衝 195 B、往返一致；追加同 offset 的項目會互相覆寫名稱。 |
| H-06 | CONFIRMED（由 plausible 升級） | medium | live PAPGT 確實有 `0036`（optional、語言 FRE，未安裝的法文群組）；沒有目錄的四項是 0036／0037／0039／0040。`docs/archive-format.md:268` 也用 `0036/` 當範例。 |
| H-07 | CONFIRMED | medium | 子項 3 較弱（同一清單第 3 步已說要替換 `meta/0.papgt`）。 |
| H-08 | CONFIRMED | low | |
| H-09 | CONFIRMED | low | pathlib 行為實測成立。 |
| H-10 | CONFIRMED | low | 實測：長度 2 的 `scale` 寫出短 4 B，重 parse 失敗。 |
| H-11 | CONFIRMED（程式碼與缺測試） | **改 nit** | live base 7,068 列與 `dlc_01` 8,218 列 Python 往返位元組一致，差異今天不出現。 |
| H-12 | CONFIRMED | low | 實測重 parse 失敗。 |
| H-13 | PARTLY | low | 過時的 `ItemInfo` 欄位是 8 個（不是 7）；stub 123 欄 vs 實際 124 欄。 |
| H-14 | CONFIRMED | low | 一處行號偏移（`python.rs:1033`）。 |
| H-15 | CONFIRMED | low | |
| H-16 | CONFIRMED | low | 實測：破壞 row 100 的一個 count，lossy 在同列 9 bytes 後重新錨定，記了 82 筆錯誤才到 row 101（只丟 1 個物品）。 |
| H-17 | CONFIRMED（由 plausible 升級） | low | 實測：`decompress_data(b'\x00', 3, 1<<40)` 讓行程直接 abort；`1<<63` 丟 `PanicException`（不是 `Exception` 子類）。 |
| H-18 | CONFIRMED（CI／死碼）／見下方 | nit | |
| H-19 | CONFIRMED | nit | |

新發現（皆併入既有 finding）：`docs/archive-format.md:268` 的 `0036/` 範例（→ H-06）；lossy 錯誤清單一處損壞記 82 筆，`len(errors)` 嚴重高估（→ H-16）；`PackGroupTreeMeta::to_bytes` 名稱緩衝長度 `as i32`（→ H-05）。

---

## I. 建置、CI、測試基礎設施、腳本與資料

| ID | 判定 | 嚴重度 | 查證重點 |
|---|---|---|---|
| I-01 | PARTLY | medium | 重算：447 個 `#[test]`、48 個 ignored → **399 個可執行，CI 只跑 111 個，288 個只在 `c_abi` 下跑**（報告寫約 397／286；72% 成立）。`c_abi` 之後的表模組是 30 個（不是 27）。與 dev-log「398」的差 1 是 `e465942` 加的 `claude_md_files_stay_under_40_kb`。主 session 實測 399 passed 吻合。 |
| I-02 | CONFIRMED | medium（可 low） | `extract_dlc_bin` 來自 `4f27d99`（本次週期新增）；`find_table_bytes` 型態來自 `a00be01`（2026-05）。 |
| I-03 | CONFIRMED | low | 6 個非 ignored 測試不看覆寫：papgt×2、pamt×2、skill_1_05、checksums。 |
| I-04 | CONFIRMED | low | 主 session 實測：`G:` 只有 `bin_backup`，測試永遠 skip；放入 1.0x install 會在 `lib.rs:437` panic。 |
| I-05 | CONFIRMED | low | private repo 有 9 份 `save.save`，`src/` 用到 6 份；沒用到的是 `editor-edit/{before,after}` 與 `engine-natural/after`。 |
| I-06 | PARTLY | low | `c_abi/character_info.rs` 有 17 處 `join("slot107")`（不是 18），幾乎都在 `#[ignore]` probe；`scripts/CLAUDE.md` 已記載 slot 固定。 |
| I-07 | CONFIRMED | nit | 另外 `scripts/CLAUDE.md:72` 說 `test_full_roundtrip` 讀 install，實際讀 `out\`。 |
| I-08 | CONFIRMED | low | |
| I-09 | CONFIRMED | low | DLL import 檢查見下方實測。 |
| I-10 | CONFIRMED | low | |
| I-11 | CONFIRMED | nit | 另外：`dev` push 與 PR 的 concurrency group 不同，dev→main PR 會跑兩次 CI。 |
| I-12 | CONFIRMED | low | encoder 已經從 c_abi 經 `Body::write` 被呼叫，「還沒有 caller」的理由過時。 |
| I-13 | CONFIRMED | nit | 實測差值剛好等於行數：root `CLAUDE.md` 18,258 vs 18,176 B（82 行）、`scripts/CLAUDE.md` 30,223 vs 30,046（177）。本機量法比 CI 嚴格。 |
| I-14 | CONFIRMED | low | |
| I-15 | CONFIRMED | low | |
| I-16 | CONFIRMED | medium（可 low） | live 2.06 的 271 + 4 個檔案攤平後沒有撞名，攤平那部分屬潛在。 |
| I-17 | CONFIRMED（潛在） | low | 目前 `out/items.jsonl` 8,379 筆沒有空清單。 |
| I-18 | CONFIRMED | nit | |
| I-19 | PARTLY | low | 報告的重現錯：pwsh 7.7 下 `gh` **不存在**會因 `$ErrorActionPreference='Stop'` 中止；只有 `gh` 存在但回非 0（例如未登入）才會繼續，而且 `:58` 會印出真實狀態。第 2–4 點成立。 |
| I-20 | CONFIRMED | nit | |
| I-21 | CONFIRMED（靜態）／NEEDS-RUN（advisory） | nit | 本機沒有安裝 `cargo-audit`／`cargo-deny`，advisory 掃描未執行。 |
| I-22 | PARTLY | low | 44 個 `#[test]` 中 36 個 ignored、8 個會跑（報告自己的清單列了 8 個卻寫 7）。 |
| I-23 | CONFIRMED | nit | |

新發現：`find_characterinfo_header`（`character_info/mod.rs:246-248`）與 `find_questinfo_header`（`quest_info/mod.rs:174-176`）用 `.unwrap_or_default()`，header 抽不出來時靜默退回 body-only 掃描（同 I-02 型態，low）；`lib.rs:243`、`:285` 的 skip 訊息印 `GAME_DIR`，但資料來自 `game_root()`（nit）。

---

## 修正排程（建議，尚未開始）

依「現在就影響使用者或下游」→「防護網」→「壞輸入防護」→「文件」排序。每一列是一個 PR 的建議範圍；依 CLAUDE.md，每個 PR 都要附 dev-log 條目，C ABI 行為改變的要註明 CrimsonAtomtic 是否需要跟進。

| 優先 | PR 範圍 | 涵蓋 finding | 理由 |
|---|---|---|---|
| P0 | **CI 跑 `cargo test --lib --features c_abi`**（加上 `--test-threads=4`），並修正 CLAUDE.md:31 vs :37 的矛盾 | A-14、D-03、E-08、I-01 | 目前 CI 只跑 399 個測試中的 111 個；之後每個修正都要靠它把關，成本最低、效益最大，先做。 |
| P1 | **Skill 2.06 buff 佈局**：tandem walk 2.05.01 vs 2.06 的 `BuffData`，修 `read_common_base`；把 live 2.06 的 raw fallback 數釘成測試上限 | **NEW-1**、C-03 | 78% 的 skill 對 Python 呼叫端失去 buff 資料，而且所有測試都綠燈。 |
| P1 | **D 區塊 2.06 header 路徑**：七個 `*_lossy_with_header` 在 `detect_named_header` 成功時直接回 header 的 `(key, name)`；characterinfo 三個查詢改成先完整比對再查 lo24，`lookup_display_name` 完整 key 命中時用完整 key 當 hi32；測試比對 `get_entry` 與 header 列數 | D-01、D-02、D-07、stage 雜訊 key、B-02 第 5 點、I 新發現（header helper 的 `unwrap_or_default`） | mission 36%、stage 6%、gimmick 1% 的名稱查不到，characterinfo 359 列顯示名稱查不到；文件引導下游走的正是這條路徑。CrimsonAtomtic 不用改呼叫方式，結果會變多。 |
| P1 | **存檔 FFI 一致性**：`list_all_items`／`list_field_positions` 取 `SAVE_FFI` lock；deferred batch 期間依契約處理 `mutation_version`；`set_object_list_present(1)` 檢查已 present；改長度的 list 操作檢查 `count == elements.len()` | G-01、G-04、G-03、G-06、G-07（補對應測試） | G-01 在 C# 端的 `Task.Run` 用法下實際可達。 |
| P2 | Dye slot 數語意：`resolve_dye_slot_count` 改名或改回傳值，文件改成「sub-mesh 數」 | E-01、E-13 新發現 | 文件把錯的數字當成權威 slot 數，下游會照著畫 slot picker。 |
| P2 | 存檔編輯安全網：transplant／insert 的 schema 檢查與 `MUTATION_INVALID` 承諾；encoder 拒絕未解完的 block（依 F 區塊查證結果定範圍） | G-02、F-01、G-10、G-11 | |
| P2 | `pack_mod` 安全：拒絕已在 PAPGT 的 group name（`0036` 實際撞到法文群組）、改寫 `0.papgt` 前備份、修正文件範例 | H-06、H-07、H-08、H-09 | |
| P3 | 不可信輸入的 abort 路徑：配置上限、遞迴深度、解壓縮輸出長度 | A-01、A-02、A-03、A-12、F-02、F-03、F-05、H-17 | 只有壞檔或惡意檔才觸發，但會讓 C# 宿主直接 abort。 |
| P3 | 測試與基礎設施：live iteminfo byte roundtrip 測試（base + `dlc_01`）、`CRIMSON_GAME_ROOT` 一致採用、`G:` 死測試、`repo_doc_budget` 改量 git 位元組、腳本覆寫保護 | C-02、C-18／I-04、I-02、I-03、I-05、I-13、I-14、I-16 | |
| P4 | 文件與註解脫節（一次處理）：`c-abi-surface.md` 補符號、過時列數與版本註解、`allow(dead_code)` 清理 | A-18、B-12、C-13～C-17、D-08～D-11、E-14～E-16、G-13、G-16、H-13～H-15、I-07、I-10～I-12 等 nit | |

**進度（2026-11-28）：** P0 與四項 P1 已在 `dev` 完成：CI 改跑 `--features c_abi`（404 個測試）；skill 2.06 inline condition（raw fallback 1,683 → 308）；D 區塊七個 `_with_header` 改讀 header 列與 characterinfo 先比對完整 key；存檔 FFI 的 G-01／G-03／G-04／G-06。細節見 [`dev-log.md`](dev-log.md)。P2 也已完成：E-01（`resolve_dye_slot_count` 改回傳真正的 dye slot 數）、F-01（encoder 拒絕未解完的 block）、G-02（每次 commit 驗證重新 decode 的結果）、G-11（新錯誤碼 −27）、G-10 的 placeholder 部分、H-06～H-09（`pack_mod` 系列）。G-10 的 scalar type name 比對需要改 CRET 格式，尚未處理。P3 也已完成：A-01（trie 迴圈與深度上限）、A-02／A-03／F-05／H-17（配置與解壓縮大小上限）、A-12（DDS 表頭上限）、F-02／F-03（TOC count 與 locator 巢狀上限）、C-02（live iteminfo base + `dlc_01` 寫回測試）、C-18／I-04（跨版本 skill 測試改讀版本封存）、I-02（漂移時測試失敗而非 skip，只改了共用 helper）、I-03、I-05、I-13、I-14、I-16。P4 也已完成：C ABI 符號目錄補齊並加測試（A-18／C-14／D-11／E-14／G-16 的符號部分）、過時的 CI／TailSizeCache／版本註解（C-01、I-07、I-10、I-11）、`CrimsonItemInfoSummary` offset 釘住（C-13）、paloc key 長度（D-08）、stale `allow(dead_code)` 與其遮住的死碼（C-17、F-14、I-12）、Python stub／版本自檢／api.md（H-13～H-15）。P5（排程表外的 medium 與 low 程式缺陷）也已完成：A-05／A-08／A-09／A-11、B-05／B-06／B-09、C-05／C-07／C-08／C-09、D-09、F-07／F-12、G-08／G-12／G-13／G-15、H-01／H-02／H-03／H-10／H-19，以及 I-02 剩下的 28 個模組 helper；跨版本 skill 測試的資料改放 `crimson-rs-private/gamedata/`。P6（剩下的 low／nit 程式缺陷）也已完成：A-04／A-06／F-06／A-13／A-14／A-15／A-17、A-10 的 flag 與 paver 部分、B-08／B-10、C-06／C-10／C-12、D-05、E-02、E-05 的錯誤回報、F-11 的 encoder 檢查、G-05、H-05／H-12、I-08／I-17、I-19 的原生指令部分；B-03 早在 `d541ce2` 已修，D-04 決定不修（PALOC 依 key 查名稱也涵蓋只在 DLC 的列）。P7（FFI 回報失敗）也已完成：B-02（每個表 loader 讀不到表時回 `BODY_PARSE`、`sibling_header` 不分大小寫並回報 I/O 錯誤、DLC merge 檢查 header）、E-04 的整表部分、F-10（HMAC 未通過的存檔預設不寫回）、A-16／C-19（`CString` 不再 `Box::leak`）、E-03。P8（其餘 low）也已完成：F-08、C-04、H-04、I-06、I-09、I-15，以及 D-06（faction）、H-16（文件）、H-18／I-11（CI）的部分。各條目前狀態以上方「修正狀態總表」為準。

**不修：** A-07（REFUTED）。**併入他項：** E-07 → A-01／A-02；B-01 → B-02。

**仍待執行的驗證：** C-01（預設執行緒數已通過一次，需多跑幾次並量記憶體峰值）、G-09（legacy-walk block 的 presence toggle）、I-21（`cargo audit`，需要先安裝 `cargo-audit`，屬於新工具，未經同意不安裝）。
