# Code audit — 區塊 C：核心大型解析器（ItemInfo / Skill / StringInfo）

- **基準：** `dev` @ `e465942`（遊戲 2.06，paver `2/6/0`）。下文所有 `檔案:行號` 都以這個 commit 為準。
- **範圍：** 計畫書 [`code-audit-2026-11-28-plan.md`](code-audit-2026-11-28-plan.md) 的 C 區塊：`src/item_info/{item,structs,keys,mod}.rs`、`src/skill_info/{mod,buff_data,post_buff,pabgh}.rs`、`src/string_info/{mod,pabgh}.rs`、`src/c_abi/{iteminfo,skill_info,string_info}.rs`。C 直接依賴的 A／B 程式碼（`binary/types.rs` 的 `CArray` / `CFlaggedArray` / `COptional` / `CString`、`binary/static_header.rs`、`binary/mod.rs` 的 `py_binary_struct!` 條件欄位）只就「對 C 的影響」討論，A／B 本身的結論見各自的報告。
- **方法：** 只做靜態程式碼、git 歷史與文件審查。**沒有修改任何程式碼、沒有建置、沒有執行任何測試、沒有派發 subagent、沒有碰 PAPGT trailer。** 依 CLAUDE.md 的規則，凡是需要執行才能證實的行為後果一律標 **plausible**；只有讀程式碼或文件就能完全判定的事實（文件和程式碼互相矛盾、某個測試不存在、某個常數的值）才標 **confirmed**。
- **來源歸屬的定義：** 基準 commit `e465942` 本身只動了文件、`src/repo_doc_budget.rs` 和 `item.rs` 的一行註解，C 範圍內的程式碼沒有任何改動，所以**沒有任何程式碼 finding 是「本次引入」**。每條 finding 都標出引入它的 commit。最近一次 C 範圍的程式碼變更是 2.06 支援（`95fa25f`，2026-11-27），由它引入的問題另外標成「近期引入」。文件類 finding 如果在 `e465942` 裡被改寫但沒有修正，會寫明。

---

## 摘要

| # | 標題 | 嚴重度 | 確信度 | 來源 |
|---|---|---|---|---|
| C-01 | `TailSizeCache` 多 GB 配置：根因已在 `a9d7012` 修正，但文件仍把它當成現存問題，而且「HashMap 順序造成非決定性」的解釋沒有程式碼依據 | low | confirmed（文件過時）／plausible（真正的非決定性來源） | 既有（文件；`e465942` 改寫該段時沿用） |
| C-02 | ItemInfo 沒有任何讀 live install 的位元組 roundtrip 測試，DLC 表連一次寫回都沒有測 | medium | confirmed | 既有（2.04 起加上 DLC 表，一直沒有測） |
| C-03 | Skill 的 raw fallback 是靜默降級，測試只印出 fallback 數量、不檢查 | low | confirmed（沒有檢查）／plausible（後果） | 既有（`7f6401a`） |
| C-04 | Skill post-buff 驗證器的硬上限（list ≤ 50、字串 ≤ 2000、必須是 UTF-8）一旦被真實資料超過，整張表就載入失敗 | low | plausible | 既有（`7f6401a`／`1f32ec2`） |
| C-05 | `detect_format` 在範圍檢查之前切片，舊版 header 的 offset 超過 body 時會 panic | low | plausible | 既有（`7f6401a`） |
| C-06 | `crimson_iteminfo_load_from_bytes` 在 2.06 上回傳 `OK` 但名稱全空，`lookup_string_key` 回 `OK` 加空字串而不是 `NOT_FOUND` | low | confirmed | 近期引入（`95fa25f`） |
| C-07 | 只有 body-only 的 iteminfo 路徑會吃到 A-11 的配置放大（有 header 的路徑以列長為上限） | low | plausible | 既有 |
| C-08 | `parse_iteminfo_with_header` 不檢查列是否緊密排列、是否涵蓋整個 body，C ABI 又跳過尾端檢查 | low | plausible | 近期引入（`95fa25f`） |
| C-09 | Skill 不比對 header key 和列內 key；2.06 重建時直接丟掉 header 的 key | low | plausible | 既有（legacy）／近期引入（2.06 部分，`95fa25f`） |
| C-10 | 存在旗標「非 0 即存在」：讀到未知值時猜測後繼續，寫回時正規化成 `1`；條件欄位在修改後不對稱 | low | confirmed（程式碼不對稱）／plausible（會在真實資料出現） | 既有（`1f32ec2` 等） |
| C-11 | StringInfo 的 C ABI 只看 body，`RowLayout::detect` 以「第一個 u32 是否為 0」判斷佈局 | low | plausible | 近期引入（`95fa25f`） |
| C-12 | `artifact_by_mission` 在 1:1 不變式被打破時，誰勝出取決於 `HashMap` 的隨機迭代順序；DLL 往 stderr 寫警告 | nit | confirmed | 既有（`72b5179` 之後） |
| C-13 | `CrimsonItemInfoSummary` 只釘大小和對齊、沒有釘欄位 offset；結構、flag 位元與 19 個 scalar getter 在 `docs/` 裡完全沒有文件；結構上方的註解寫錯大小 | low | confirmed | 既有（`2b1307a`） |
| C-14 | `docs/c-abi-surface.md` 沒有逐一列出這三個 bridge 的 `extern "C"` 符號 | low | confirmed | 既有 |
| C-15 | 過時與自相矛盾的註解（目錄名稱、版本、gem 定義、slot 數、BString） | nit | confirmed | 既有 |
| C-16 | `quick_slot_index` 從 2.04 起恆為 0，getter 卻回 `OK`；`IS_IMPORTANT_ITEM` 是從三值欄位推導的 | nit | confirmed | 既有（`1f32ec2`） |
| C-17 | `item_info` / `string_info` 無條件的 `allow(dead_code)` 抵銷了 `lib.rs` 依賴的全 feature dead-code 檢查 | nit | confirmed | 既有 |
| C-18 | `test_skill_roundtrip_cross_version` 在封存存在時必定失敗；`test_skill_roundtrip_1_05` 名稱與實際讀的資料不符 | low | plausible | 既有（2.01／2.04 之後失效） |
| C-19 | 非 UTF-8 的 `CString` 會 `Box::leak`，在長駐的 C# 編輯器裡每次載入都洩漏 | nit | plausible | 既有 |

---

## `TailSizeCache` 專章（審計重點 1）

計畫書把這條列為「目前已知最具體的穩定性問題」，要求找出根因。結論如下：

1. **快取不跨表、不跨測試、不跨格式共用。** `TailSizeCache` 是 `HashMap<u8, usize>`（`src/skill_info/buff_data.rs:34`），只在兩個地方建立，而且都是每次 parse 呼叫的區域變數：`SkillData::parse_legacy`（`src/skill_info/mod.rs:140`）與測試用的 `probe_entry_failures`（`mod.rs:102`）。沒有 `static`、沒有 `thread_local`，也沒有傳到 skill 以外的地方（全 repo grep `TailSizeCache` 只命中 `skill_info`）。格式（`SkillFormat`）在建立快取**之前**就由 `detect_format` 決定（`mod.rs:138`），`detect_format` 本身不用快取，所以同一個快取裡不會混入兩種格式的 tail size。**confirmed。**
2. **快取只做 `get` / `insert`，從不迭代。** `mod.rs:392`、`:406`、`:493`、`:524`。修正前的版本（`a9d7012^`）也一樣（`git grep` 確認）。`HashMap` 的隨機種子只影響迭代順序，不影響 `get` 的結果，所以**這個快取不可能是非決定性的來源**。**confirmed。**
3. **真正的多 GB 配置根因已經修掉了。** `a9d7012`（2026-11-07，"fix(skill): refuse list counts that cannot fit, instead of allocating them"）之前，`read_common_base` 的 `field_96_list` / `field_112_list` 以及 `post_buff::read_list_u32`，會把探測時在錯誤 offset 讀到的任意 `u32` count 直接交給 `Vec::with_capacity`。`0xEB000000 × 4 B` 就是 15.7 GB。現在三處都改走 `post_buff::read_count`（`src/skill_info/post_buff.rs:134-147`），count 超過「剩餘位元組 / 元素大小」就回 `InvalidData`；回歸測試是 `buff_data.rs:244-259`、`post_buff.rs:413-421`。我逐一檢查了 skill 現在所有的配置點：`level_count ≤ 100`、`buff_count ≤ 200`（`mod.rs:367`、`:376`），`field_48` / `read_var_bytes` / `read_name_bytes` 都先比對剩餘長度再 `to_vec`，`try_parse_remaining_cache_only` 的迴圈不配置記憶體，每圈至少前進 1 個位元組。**skill 解析器現在沒有不受上限約束的配置。**（結構推論是 confirmed；「修正有效」需要執行回歸測試，屬於 plausible。）
4. **「非決定性」比較可能的解釋**（plausible）：修正前，對同一份輸入，那個 15–17 GB 的 `with_capacity` **每次都會發生**（parse 是決定性的）。在 Windows 上，`with_capacity` 不會碰觸頁面，只會佔用 commit charge，所以成不成功取決於當下系統還剩多少 commit 額度（RAM 加 pagefile，再扣掉其他平行測試的用量）。失敗時 Rust 呼叫 `handle_alloc_error` → `abort` → `__fastfail`，在 Windows 上顯示為 `STATUS_STACK_BUFFER_OVERRUN`，和 1.18 的紀錄完全吻合。1.17 的紀錄也是：`memory allocation of 17115381760 bytes failed` 之後「立刻重跑就 2013/2013 ok」，而 skill 檔案和 1.16 位元組相同。輸入相同、結果不同，符合「額度」而不符合「雜湊順序」。執行緒數越多，同時佔用的 commit 越多，這也解釋了為什麼 `--test-threads=4` 有效。
5. **剩下的問題是文件**，見 C-01。

---

## Findings

### C-01 — `TailSizeCache` 的文件過時，根因解釋也不正確

- **位置：** `CLAUDE.md:37`（「the default thread count can trip a non-deterministic `TailSizeCache` multi-GB allocation」，以及 probe「can hit the same allocation artifact — re-run before concluding anything」）、`CLAUDE.md:69`（「the `TailSizeCache` balloon」）、`scripts/CLAUDE.md:72`、`scripts/CLAUDE.md:80`（「it is non-deterministic via the `TailSizeCache` hash order … just re-run」），以及 `a9d7012` 的 commit message（「Whether it fired depended on TailSizeCache's HashMap order」）。
- **問題與根因：** 見上方專章。根因（未檢查的 count 拿去配置）在 2026-11-07 已經修正，`a9d7012` 的 commit message 也寫了「default thread count, three runs: 372 passed, 0 failed each」。但這四處文件在修正之後還被 `5e69ec6`（2026-11-21）和基準 `e465942`（2026-11-28）改寫過，都保留了「現存、非決定性、重跑就好」的說法。另外，「HashMap 順序」這個機制在修正前後的程式碼裡都不存在（快取從不迭代）。
- **風險：** 「先重跑再下結論」這條指示，會讓下一次真正的回歸（例如新加的 list 讀取又忘了走 `read_count`）被當成已知雜訊而忽略。`--test-threads=4` 本身無害，但它現在的理由已經不成立；還有沒有其他記憶體壓力需要它，**沒有驗證過**。
- **可重現的情境：** 讀 `git show a9d7012` 與上述四處文件即可對照。要驗證「已經不需要」，在 live install 上以預設執行緒數連跑幾次 `cargo test --lib --features c_abi`，觀察 commit charge 的峰值。
- **嚴重度：** low。
- **確信度：** confirmed（文件與程式碼／commit 不一致）；plausible（第 4 點的 commit-charge 解釋，沒有重現）。
- **來源：** 既有（文件；`a9d7012` 之後未更新，`5e69ec6` 與 `e465942` 改寫時沿用）。

### C-02 — ItemInfo 沒有讀 live install 的位元組 roundtrip 測試，DLC 表沒有任何寫回測試

- **位置：**
  - `src/lib.rs:121-124`、`:150-167`（`test_full_roundtrip`）：讀的是 `out\iteminfo.pabgb`，這是 `scripts/export_for_ce.py` 匯出的檔案，不是 live install，也不包含 DLC。
  - `src/item_info/item.rs:730-764`：`test_parse_first_item` / `test_parse_with_header` 也讀 `out\`，而且只做解析、不寫回；`:763` 斷言「所有 `row_index == 位置`」，這只對 base 表成立（DLC 不成立，見 `item.rs:20`）。
  - DLC：`iteminfo.dlc_01` 唯一被讀到的地方是 E 區塊的 `src/c_abi/item_part_prefab.rs:2324-2326`，用 body-only 路徑 `unwrap()` 每一列。這證明每一列都能被**完整消耗**，但沒有寫回比對，也沒有走 `parse_iteminfo_with_header`（key 與 header 的比對、`off == row.len()` 的檢查都沒跑過 DLC）。`out\iteminfo.dlc_01.pabgb` 存在，但 `src/` 裡沒有任何程式碼讀它。
  - `scripts/export_for_ce.py:157-165` 的 `ok` 定義是 `consumed == len(chunk)`，是「消耗完整」而不是「寫回相同」。
- **問題與根因：** 專案規則是「讀寫必須位元組一致」，CLAUDE.md 也宣稱 2.06 在 7,068 + 8,218 列上 byte-perfect，但 CI 和 `cargo test` 沒有任何測試從 live install 驗證這件事，DLC 表更是連一次寫回都沒有。消耗完整的檢查抓不到 A-10 那類「讀得進來、寫回不同」的問題，例如 `CFlaggedArray` 的 `01 00000000`（見 C-10）。2.04 的教訓正是「只有 DLC 表才會出現的欄位值」，而 DLC 表正好是測試最薄弱的地方。`scripts/CLAUDE.md:72` 寫「The iteminfo / skill roundtrip tests are pinned to the local game install and skip cleanly when the install isn't on the expected version」，兩句都不正確：iteminfo 讀的是 `out\`（`validated-patches.md` 的 2.04.08 列也記載過「two tests reading the stale `out/iteminfo.pabgb`」而失敗）；skill 測試在 parse 失敗時會 `panic`（`lib.rs:396-397`），不會 skip。
- **可重現的情境：** 在任何一列 DLC 資料裡，把某個空的 `CFlaggedArray` 從 `00` 改成 `01 00 00 00 00`：`export_for_ce.py` 仍然回報 `ok`，C ABI 照常載入，所有 Rust 測試都是綠的；只有寫回比對才會發現少了 4 個位元組。
- **嚴重度：** medium（直接關係到專案的核心規則，而且盲點正好落在已知會出問題的 DLC 表上）。
- **確信度：** confirmed（測試涵蓋範圍是讀程式碼就能確定的事實）。
- **來源：** 既有（`test_full_roundtrip` 一直讀 `out\`；DLC 表從 2.04 起出現，一直沒有補上）。

### C-03 — Skill 的 raw fallback 是靜默降級，測試不檢查

- **位置：** `src/skill_info/mod.rs:297-312`（buff matrix 解不開就把整段存成 `buff_raw_fallback`）、`:316-330`（post-buff 對不上時，連已經解好的 matrix 也降級）、`:612`（`detect_format` 找不到任何可行的格式時，靜默回傳 `WithField58`）；`src/lib.rs:398-409`（`run_skill_roundtrip` 只 `println!` raw fallback 的數量）。
- **問題與根因：** raw fallback 是刻意的設計（保住 roundtrip），但沒有任何地方釘住「目前 live 資料的 fallback 數量應該是多少」，`docs/` 裡也沒有紀錄這個基準值（grep `raw fallback` 只命中 Python 存根和 `python.rs`）。如果下一個 patch 在 `BuffData` 的共同 base 裡插一個欄位，或者快取了一個錯誤的 tail size，每一列都會退成 raw blob，parse 成功、位元組 roundtrip 成功、測試全綠，但 Python API（`python.rs:1389-1391`）拿到的 `buff_level_list` 全部變成 `None`。C ABI 只用名稱，不受影響。這違反跨區塊的「版本漂移要明確失敗」原則，只是影響範圍限於 Python 使用者。
- **可重現的情境：** 讓 `read_common_base` 多讀一個 `u8`，模擬漂移：預期 `test_skill_roundtrip_1_05` 仍然通過，只有它印出的 `raw fallback=` 數字會改變。
- **嚴重度：** low。
- **確信度：** confirmed（測試沒有檢查）；plausible（漂移時實際會全部退成 fallback，沒有實作驗證）。
- **來源：** 既有（`7f6401a`）。

### C-04 — post-buff 驗證器的硬上限可能讓整張 skill 表載入失敗

- **位置：** `src/skill_info/post_buff.rs:344-363`（`try_parse_post_buff_end`：list count `> 50` 就拒絕、字串 `> 2000` 位元組就拒絕、非 UTF-8 也拒絕）。它被三個地方使用：`probe_subclass_tail` → `try_parse_remaining_cache_only`（`mod.rs:533`）、`find_post_buff_start`（`mod.rs:542`）、`detect_format`（`mod.rs:602`）。嚴格的讀取器 `read_post_buff_tail`（`post_buff.rs:257-286`）沒有這些上限。
- **問題與根因：** 驗證器的上限沿用 Python 的啟發式，比真實格式嚴格。假設某個 patch 讓一列的 `usable_character_info_list` 有 51 個元素，或者 `dev_skill_desc` 超過 2000 位元組（約 666 個韓文字）。如果這一列剛好需要探測一個還沒快取的 `type_id`，或者它的 buff matrix 解不開，那麼探測和 `find_post_buff_start` 都會失敗，`parse_body` 回 `could not locate post-buff boundary`（`mod.rs:304-309`），`SkillData::parse` 就在第一個錯誤處整張表放棄（`mod.rs:152-158`）。C ABI 回 `BODY_PARSE`，編輯器拿不到任何技能名稱。結果還取決於列的順序：同樣的長 list 如果出現在所有 `type_id` 都已經快取之後的列，就不需要探測，只走嚴格讀取器，可以成功。這屬於「明確失敗」（好），但是是「一列拖垮整張表」的懸崖；`_probe_skill_entry_failures` 會把它顯示為「單一新技能」，診斷流程正確。
- **可重現的情境：** 構造一列 skill，它的第一個 buff 用一個沒出現過的 `type_id`，並把 `usable_condition` 的 count 設成 51（元素補齊）：預期整個 `SkillData::parse` 回錯。
- **嚴重度：** low。
- **確信度：** plausible。
- **來源：** 既有（上限來自 `7f6401a`，2.04 重排時 `1f32ec2` 保留）。

### C-05 — `detect_format` 在範圍檢查之前切片，可能 panic

- **位置：** `src/skill_info/mod.rs:571`（`let entry = &pabgb[*start..*end];`），由 `:138` 呼叫，早於 `:142-151` 的 `start > end || end > pabgb.len()` 檢查。`probe_entry_failures` 也一樣：`:100` 先呼叫 `detect_format`，`:105` 的防護根本輪不到。
- **問題與根因：** `parse_pabgh`（`pabgh.rs:26-62`）只驗證長度、不驗證 offset；`entry_ranges`（`pabgh.rs:87-104`）會讓按 offset 排序後的最後一列以 `pabgb_len` 作為結尾。只要有任何 offset 大於 body 長度，就會得到 `start > end`，在 `detect_format` 裡 panic。2.06 路徑不會觸發：`to_legacy_layout` 重建出的 offset 是連續的。但只要 `to_legacy_layout` 回傳 `None`，就會走舊版路徑，例如舊版 header，或者任何 `detect_named_header` 不認得的 header。
- **可重現的情境（逐位元組追蹤，未執行）：** `pabgh = 01 00 | 01 00 00 00 | FF 00 00 00`（count 1、key 1、offset 255），`pabgb` = 10 個 `00`。`detect_named_header` 對全部 12 組寬度都回 `None`（count 對 10 位元組的 header 而言都太大，或者無法剛好消耗完）→ `parse_legacy` → `parse_pabgh` 通過（2 + 8 = 10）→ `entry_ranges` = `[(255, 10)]` → `&pabgb[255..10]` panic。C ABI 有 `catch_unwind`（`src/c_abi/skill_info.rs:142-160`），而且專案沒有設 `panic = "abort"`（`Cargo.toml`），所以結果是回傳 `error::PANIC` 而不是 `BODY_PARSE`，並在 stderr 印出 panic 訊息；Python 端會得到 `PanicException`。不會造成 UB。
- **嚴重度：** low。
- **確信度：** plausible（靜態追蹤到 panic 位置，沒有執行）。
- **來源：** 既有（`7f6401a`）。

### C-06 — iteminfo body-only 載入器在 2.06 上靜默回傳空名稱

- **位置：** `src/c_abi/iteminfo.rs:556-582`（`crimson_iteminfo_load_from_bytes`，文件 `:549-553` 沒有提到 2.06）、`:323-327` 與 `:353`（沒有 header 時逐列 `ItemInfo::read_from`，`string_key` 是 `HeaderName`，讀不到任何位元組，永遠是空的，見 `src/binary/types.rs:106-110`）、`:356`、`:469`（空名稱照樣放進 `by_key`）、`:683-714`（`lookup_string_key`：key 存在就回 `OK`，`*required = 1`，寫出一個空字串）。
- **問題與根因：** 2.06 把名稱移到 header，但 body-only 入口仍然成功載入，並且對每個 key 回答「名稱是空字串」，而不是 `NOT_FOUND` 或載入失敗。`lookup_string_key` 自己的文件寫的是「Returns `NOT_FOUND` when `item_key` doesn't match」，沒有提到空名稱這種情況。`docs/c-abi-surface.md:58-61`、`:71-74` 有說明，也要求下游改用 `_with_header`，所以這是「有文件的靜默降級」；但函式本身的 doc comment 沒有提醒，呼叫端如果只看標頭檔，不會知道。`load_from_bytes_with_header` 在 `header_len == 0` 時也會悄悄退回 body-only（`:325` 的 `!h.is_empty()`）。
- **可重現的情境：** live 2.06 base 表，`crimson_iteminfo_load_from_bytes` → `OK`；`crimson_iteminfo_lookup_string_key(h, 2200, …)` → `OK`，`required == 1`。
- **嚴重度：** low。
- **確信度：** confirmed（程式碼路徑完全確定）。
- **來源：** 近期引入（`95fa25f`，2.06 把 `string_key` 改成 `HeaderName`）。

### C-07 — body-only 路徑才會吃到 `CArray` 的配置放大（A-11 在 C 的具體情況）

- **位置：** `src/binary/types.rs:144-154`（`count <= remaining`，但實際配置的是 `count × size_of::<T>()`）；`src/c_abi/iteminfo.rs:353`（body-only：`data` 是**整個** body，約 6 MB）對照 `src/item_info/item.rs:698-702`（有 header：`row` 只是**單一列**的切片）。
- **問題與根因：** A-11 已經指出單位不對。C 這邊的補充是：有 header 的路徑把 `remaining` 限制在一列的長度內（通常數百位元組到數 KB），所以最壞情況只是幾 MB；body-only 路徑（C ABI 的 `load_from_bytes`、`load_from_file` 沒有 sibling header 時、`lib.rs` 與 E 區塊的測試）的 `remaining` 是整個 body。`DockingChildData`（三個 `CString`，每個 40 位元組，再加上 `[u32; 4]` ×2 等欄位）或 `InspectData` 的 `size_of` 估計在 150–250 位元組（**估算，沒有用編譯器量**），6 MB × 200 ≈ 1.2 GB。這是一次 commit charge 尖峰，失敗時會走上 C-01 那條 `abort` 路徑，而且 `catch_unwind` 擋不住。只有損壞或惡意的輸入才會觸發；正常的 live 資料不會。
- **可重現的情境：** 在 body 的第一列，把 `docking_child_data_list` 的旗標設成 `01`，count 設成接近 body 剩餘長度：預期先配置約 1 GB，然後在讀第一個元素時失敗。
- **嚴重度：** low。
- **確信度：** plausible。
- **來源：** 既有（`CArray` 的檢查依 A-11 來自 `b038c2d`；body-only 入口更早）。

### C-08 — 有 header 的 iteminfo 解析不驗證列是否完整涵蓋 body

- **位置：** `src/item_info/item.rs:694`（直接用 `parse_named_header(header, 2, 4)`，沒有呼叫 `rows_are_packed`，見 `src/binary/static_header.rs:104-113`）；`src/c_abi/iteminfo.rs:348`（有 header 時把 `offset` 設成 `data.len()`，於是 `:459-468` 的「解析結束位置 == 檔案大小」檢查永遠通過）。
- **問題與根因：** 只要每一列各自落在 body 內、各自被完整消耗、key 相符，就算列與列之間有空隙、互相重疊、同一列出現兩次，或者 body 尾端還有 header 沒列出的資料，都會被接受。同一個 crate 的 `detect_named_header`（B 區塊）會檢查 packed，但 iteminfo 沒用它。結果可能是重複的物品，或者少了物品卻沒有任何錯誤。
- **可重現的情境：** 拿 live 的 header，刪掉最後一列的 entry 並把 count 減 1：預期 `load_from_bytes_with_header` 回 `OK`，`entry_count` 少 1，body 最後一列被靜默忽略。
- **嚴重度：** low。
- **確信度：** plausible。
- **來源：** 近期引入（`95fa25f`）。

### C-09 — Skill 不比對 header key 和列內 key

- **位置：** `src/skill_info/mod.rs:217-239`（`parse_skill_entry` 從列內讀 `key`）對照 `:193-196`（`write` 在 header 裡寫 `idx.key`，在列內寫 `entry.key`）；`src/binary/static_header.rs:153-155`（`to_legacy_layout` 從 **body 列**取 key 來建 legacy header，`r.key`，也就是 header 的 key，被丟棄）。對照組：iteminfo 有檢查（`item.rs:710-715`）。
- **問題與根因：** 舊版路徑上，header 和列的 key 不一致時，兩邊各自保存、寫回時各自還原，roundtrip 沒問題，但 `index_order[i].key` 與 `entries[i].key` 會互相矛盾，而且沒人檢查。C ABI 用的是 `entry.key`（`c_abi/skill_info.rs:58`）。2.06 路徑上，header 的 key 在重建時直接消失，寫回會改成 body 的 key，roundtrip 不一致，也不報錯。
- **可重現的情境：** 把 2.06 skill header 某一列的 key 改掉（body 不動）：預期 parse 成功，`write()` 產生的 header 帶的是 body 的 key，與輸入不同。
- **嚴重度：** low。
- **確信度：** plausible。
- **來源：** 既有（舊版部分，`7f6401a`）；近期引入（2.06 部分，`95fa25f`）。

### C-10 — 「非 0 即存在」的旗標與條件欄位：審計重點 2 的答案

- **位置：** `src/binary/types.rs:222-229` / `:232-243`（`CFlaggedArray`）、`:280-290` / `:292-301`（`COptional`）、`:401-419`（`OptLocalizableString`：category 非 0 就讀 index 和 default）；`src/binary/mod.rs:133-141`、`:198-201`（條件欄位只在讀取時評估條件，寫回時看 `Option` 是否存在）；`src/item_info/item.rs:650`（`unk_pre_gimmick_visual: u32 => equip_type_info.0 != 0 || item_type == 74`）。
- **問題與根因：**
  1. **Variant（明確失敗，正確）：** `SubItem`（`src/item_info/structs.rs:615-639`）與 `SealableItemInfo`（`:754-779`）遇到未知 tag 都回 `InvalidData`。skill 的 `BuffData` flag 非 0 視為 null entry（`mod.rs:384-388`），這是格式本身的定義。
  2. **存在旗標（猜測後繼續）：** `ItemInfo` 有 24 個 `CFlaggedArray` 和 7 個 `COptional` 欄位。旗標讀到 `0x02..0xFF` 時，一律當作存在並繼續讀。漂移若落在旗標位置，通常會在 `off != row.len()`（`item.rs:704`）或下一個 count 的上限檢查被攔下，但不保證。寫回時旗標正規化成 `1`，`CFlaggedArray` 的「存在但為空」（`01 00000000`）會寫成一個 `00`。這是 A-10 的同一個問題；`EnchantDataList` 已經因為遊戲真的寫 `01` + count 0 而特別保存旗標（`structs.rs:133-142`），可見這種形狀在遊戲資料裡確實存在，這 24 個 list 沒有同樣的保護。
  3. **條件欄位在修改後不對稱：** 讀取看 `equip_type_info` / `item_type`，寫回只看 `Option`。修改 `equip_type_info`（0 ↔ 非 0）但沒有同步 `unk_pre_gimmick_visual` 的 `Some/None`，寫出的位元組重新讀入時會錯位 4 個位元組。no-op roundtrip 永遠測不到這種情況（跨區塊「Roundtrip」關注點）。
- **可重現的情境：** 2 是 A-10 的情境；3：解析一個 `equip_type_info == 0` 的物品，把它改成非 0 後寫回，再重新解析，預期從 `unk_pre_gimmick_visual` 開始錯位。
- **嚴重度：** low。
- **確信度：** confirmed（程式碼的讀寫不對稱）；plausible（真實資料或真實的編輯流程會碰到）。
- **來源：** 既有（`CFlaggedArray` / `OptLocalizableString` 來自 `1f32ec2`；`COptional` 與條件欄位更早）。

### C-11 — StringInfo C ABI 用「第一個 u32 是否為 0」判斷佈局

- **位置：** `src/string_info/mod.rs:129-134`（`RowLayout::detect`）、`:142-143`（`parse_pabgb` 用它）；`src/c_abi/string_info.rs:40-48`（C ABI 只走 `parse_pabgb`，刻意不讀 pabgh，見 `:9-14`）。
- **問題與根因：** 2.06 的列以 hash 開頭，舊版的列以值為 0 的名稱長度開頭。如果 2.06 表（或某個 DLC 的 `stringinfo`）第一列的 hash 剛好是 0，整張表就會被判成 Legacy：每一列的 `hash` 和 `row index` 互換，所有查詢都以 row index 當 key，不會報錯。`parse_pair`（有 header）用 `index.named` 決定佈局，並且逐列比對 hash（`mod.rs:174`、`:212-236`），沒有這個問題。目前 live 第一列的 hash 是 `0x04f6d06d`（`mod.rs:586`），所以只是潛在問題。
- **可重現的情境：** 構造一個 2.06 stringinfo body，讓第一列的 hash 為 0：`crimson_string_info_lookup_by_hash(h, <真正的 hash>)` 回 `NOT_FOUND`，`lookup_by_hash(h, 1)` 回第 1 列的字串。
- **嚴重度：** low。
- **確信度：** plausible。
- **來源：** 近期引入（`95fa25f`）。

### C-12 — `artifact_by_mission` 的勝出者取決於 HashMap 的迭代順序

- **位置：** `src/c_abi/iteminfo.rs:478-488`。
- **問題與根因：** 反向表是迭代 `look_detail_mission_by_key: HashMap`（`RandomState`，每個行程的種子都不同）建出來的。註解說 1:1 被打破時「keeping last-write-wins」，但「last」是隨機的，所以同一份資料在不同次執行裡可能回答不同的物品。另外，DLL 在 GUI 程式裡 `eprintln!`，通常不會被看到。live base 表有測試釘住 1:1（`:2306-2326`），所以目前不會觸發。
- **可重現的情境：** 構造兩個物品指向同一個 mission：多次執行 `lookup_artifact_for_mission`，結果在兩個物品之間變動。
- **嚴重度：** nit。
- **確信度：** confirmed（迭代順序的性質）。
- **來源：** 既有（`72b5179` 之後的反向表）。

### C-13 — `CrimsonItemInfoSummary` 的 layout 防護不完整，也沒有文件（審計重點 5）

- **位置：** `src/c_abi/iteminfo.rs:180-207`（結構）、`:209-222`（`const` assert：`size_of == 80`、`align_of == 8`）、`:172-174`（註解寫「`_reserved: [u8; 5]` brings the size up to a clean 72 bytes … asserted in the unit tests」）、`:33-65`（28 個 `CRIMSON_ITEMINFO_FLAG_*` 位元）、`:1131-1217`（19 個 scalar getter）、`:1235-1252`（`lookup_summary`）。
- **問題與根因：**
  1. 有 build-time 的大小與對齊檢查（好），但沒有 `offset_of!` 檢查。交換兩個同樣大小的欄位（例如 `item_type` 和 `item_tier`），或者把 `u16` 欄位重新排序，大小不變，C# 那邊會靜默讀錯。
  2. `:172-174` 的註解和實際的結構（`_reserved: u8`，80 位元組，由 `const` 檢查而不是 unit test）互相矛盾；`:214-219` 的註解才是對的。
  3. 在 `docs/` 裡 grep `CrimsonItemInfoSummary`、`lookup_summary`、`lookup_flags`、`CRIMSON_ITEMINFO_FLAG` 都沒有結果，只出現在計畫書裡。C# 要對齊的 80 位元組 layout 和 28 個位元的定義，只存在於 Rust 原始碼裡。
- **可重現的情境：** 交換 `item_type` 與 `item_tier` 兩行：`const` 檢查照樣通過。
- **嚴重度：** low。
- **確信度：** confirmed。
- **來源：** 既有（`2b1307a`）。

### C-14 — `docs/c-abi-surface.md` 沒有逐一列出這三個 bridge 的符號

- **位置：** `docs/c-abi-surface.md:431-445`（module map）、`:58-61`（只提到 `crimson_iteminfo_load_from_bytes_with_header`）。
- **問題與根因：** 跨區塊要求「`docs/c-abi-surface.md` 列出的函式與實際 `extern "C"` 符號一一對應」。實際符號：iteminfo 有 `load_from_file`、`load_from_bytes`、`load_from_bytes_with_header`、`free`、`entry_count`、`lookup_string_key`、`lookup_icon_path_hash`、`lookup_max_stack`、`lookup_look_detail_mission_info`、`lookup_artifact_for_mission`、`lookup_socket_caps`、`socket_allows_gem`、`socket_allowed_gem_count`、`socket_allowed_gem_at`、`canonical_gem_count`、`canonical_gem_at`、`lookup_flags`、19 個 `lookup_<scalar>`、`lookup_summary`、`get_entry`。skill 有 `load_from_file`、`load_from_bytes`、`free`、`entry_count`、`lookup_string_key`、`get_entry`。stringinfo（前綴是 `crimson_string_info_`，不是其他 bridge 的 `crimson_<table>info_` 形式）有 `load_from_file`、`load_from_bytes`、`free`、`entry_count`、`lookup_by_hash`、`get_entry`。文件只用一行概括各 bridge，iteminfo 的 flags / summary / scalar getter / `get_entry` 都沒有提到，skill 和 stringinfo 各只有「SkillKey → entry name」「u32 hash → string」。
- **可重現的情境：** 對照 `grep -n 'pub unsafe extern "C" fn' src/c_abi/{iteminfo,skill_info,string_info}.rs` 與 `c-abi-surface.md`。
- **嚴重度：** low。
- **確信度：** confirmed。
- **來源：** 既有。

### C-15 — 過時與自相矛盾的註解

全部 confirmed、nit、既有：

- `src/c_abi/iteminfo.rs:3-4`、`src/c_abi/skill_info.rs:4-5`、`src/c_abi/string_info.rs:3-4`：目錄寫成 `gamedata/binary__/client/bin/`，2.01 起是 `gamedata/binarystaticinfo__/bin`。
- `src/c_abi/iteminfo.rs:10-15`：「only the `(key, string_key)` pair is retained — the other 100+ fields are dropped」，現在還保留了 max stack、icon、mission 正反向、socket caps、允許的 gem、canonical gem 與 summary 等索引。
- `src/c_abi/iteminfo.rs:383-385`：「widened it to 8 slots」，實際是 12（`item.rs:681`）。
- `src/c_abi/iteminfo.rs:1028-1034`、`:1043-1044`：canonical gem set 寫成「sorted-ascending union of every item's `socket_item_list`」，程式碼是 `item_type == 74 && category_info == 2501`（`:455-457`），handle 的欄位註解（`:280-299`）才是對的。
- `src/c_abi/iteminfo.rs:911-913`、`:990-992`：寫成「allowed-gem list (`DropDefaultData::socket_item_list`)」、「Order matches `socket_item_list` on disk」，實際是 `socket_item_list` 加上 `add_socket_material_item_list` 的聯集（`:436-452`）。
- `src/c_abi/skill_info.rs:6-7`、`:51`，以及 `CLAUDE.md:53`：skill 名稱寫成「BString-shape（u32 len + bytes + NUL）」。2.04 之後名稱是 `u32 len + bytes`，沒有 NUL（`src/skill_info/mod.rs:45`、`:623-640`）；過去被當成 NUL 的那個位元組，現在是 `EntryHead::unk_post_key`（`post_buff.rs:194-196`）。
- `src/c_abi/skill_info.rs:22`、`src/c_abi/string_info.rs:16`：1.06 的列數與記憶體估計。
- `src/item_info/item.rs:726`、`:742`，`src/lib.rs:117-120`、`:151`：「parser targets Crimson Desert 1.05」、「1.04 baseline binary」。
- `src/c_abi/part_prefab_dye_slot_info.rs:394-395`（E 區塊，審計重點 4 順帶）：`lookup_slot_mask_full` 寫「12 bytes on 2.01+, 3 on 1.07-2.00」，但 `mask` 已經是固定的 `[u8; 12]`（`src/part_prefab_dye_slot_info/mod.rs:123`），這個解析器只讀最新版，回傳值永遠是 12。

### C-16 — 已經不存在的欄位仍以 `OK` 回傳

- **位置：** `src/c_abi/iteminfo.rs:391-392`（`quick_slot_index: 0`，「Removed from iteminfo in 2.04」）、`:1161-1165`（`crimson_iteminfo_lookup_quick_slot_index` 照常回 `OK`）；`:87-91`（`IS_IMPORTANT_ITEM` = `item_importance_kind == 0`，而這個欄位有 0 / 1 / 2 三種值，見 `item.rs:533-535`）。
- **問題：** 呼叫端無法分辨「值是 0」和「這個欄位已經不存在」；這只寫在 Rust 的行內註解裡。`IS_IMPORTANT_ITEM` 把唯一一個值為 1 的物品歸類成「不重要」，這個語意沒有驗證過。
- **嚴重度：** nit。**確信度：** confirmed。**來源：** 既有（`1f32ec2`）。

### C-17 — 無條件的 `allow(dead_code)` 抵銷了 dead-code 檢查

- **位置：** `src/item_info/mod.rs:1-6`（`item`、`keys`、`structs` 三個模組都有 `#[allow(dead_code)]`）、`src/string_info/mod.rs:52` 與 `src/string_info/pabgh.rs:21`（`#![allow(dead_code)]`）。
- **問題：** `src/lib.rs:1-15` 的設計是「只有在 `c_abi,python` 全 feature 組合下才讓 clippy 抓 dead code」。這三個無條件的 `allow` 讓 C 區塊最大的三個檔案連全 feature 組合都不會被檢查，違反跨區塊「`cfg_attr(allow(dead_code))` 不能掩蓋真正的 dead code」。
- **嚴重度：** nit。**確信度：** confirmed。**來源：** 既有（`06117ef`／`56a57da7` 起）。

### C-18 — Skill 的跨版本測試在封存存在時必定失敗

- **位置：** `src/lib.rs:378-392`（`test_skill_roundtrip_cross_version`）、`:419-452`（`extract_skill_from_archive`）、`:326-335`（`test_skill_roundtrip_1_05`）。
- **問題與根因：** 這個測試讀 `G:\我的雲端硬碟\temp\Crimson Desert\{1.03.01,1.04.01,1.05.01}\0008`，但目錄與檔名用的是 `gamedata_layout::bin_dir()` / `header()` / `body()`，而它們是對 **live install**（2.06）解析出來的（`src/binary/gamedata_layout.rs:74-86`）。在 1.0x 的 PAMT 裡找 `gamedata/binarystaticinfo__/bin` 會在 `:437` `expect("dir not found")` panic；就算找得到，只讀最新版的解析器也解不了 2.04 之前的列頭。所以只要 G: 的封存存在，這個測試就會失敗；沒有封存的機器上它永遠 skip，看起來是綠的。另外，`test_skill_roundtrip_1_05` 的名稱和 `run_skill_roundtrip` 的 label 都寫「1.05」，實際讀的是 live install（2.06）；而且它用寫死的 `GAME_DIR`（`:116`），`gamedata_layout` 卻讀 `CRIMSON_GAME_ROOT`，兩者可能指向不同的安裝（I 區塊的 `GAME_DIR` 待查事項）。最後，Machine notes 記載 G: 的封存在 `bin_backup\<ver>\`，而且是攤平的 bin 檔，不是 `<ver>\0008\0.pamt`，所以這個路徑很可能根本不存在，測試等於永遠 skip。
- **可重現的情境：** 在有 `G:\…\1.03.01\0008\0.pamt` 的機器上執行 `cargo test --lib test_skill_roundtrip_cross_version`。
- **嚴重度：** low。
- **確信度：** plausible（沒有執行；G: 的實際目錄結構沒有在這台機器上確認）。
- **來源：** 既有（2.01 改目錄、2.04 改列頭之後就失效了）。

### C-19 — 非 UTF-8 的 `CString` 會洩漏記憶體

- **位置：** `src/binary/types.rs:33-41`、`:86-90`（`Box::leak`）。
- **問題：** 註解說明的前提是「pabgb data lives for the duration of parsing」，但 C ABI 的 handle 由長駐的 C# 編輯器反覆載入與釋放；每次載入時，每個非 UTF-8 的字串都會洩漏一份副本，`crimson_iteminfo_free` 也收不回來。2.06 的資料裡有沒有非 UTF-8 字串**沒有驗證**；如果沒有，就不會發生。
- **嚴重度：** nit。**確信度：** plausible。**來源：** 既有（A 區塊的型別，C 是主要的觸發者）。

---

## 審計重點逐條對照

| # | 審計重點 | 結論 |
|---|---|---|
| 1 | `TailSizeCache` 多 GB 配置 | 快取每次 parse 重建、不跨表不跨測試、從不迭代；根因（未檢查的 count 拿去配置）已由 `a9d7012` 修正；skill 現在所有的配置都有上限。剩下的是文件與根因解釋錯誤（C-01）。 |
| 2 | `Variant` / `optional<T>` 遇到未知 tag | Variant（`SubItem`、`SealableItemInfo`）明確回錯；`COptional` / `CFlaggedArray` / `OptLocalizableString` 非 0 即存在，猜測後繼續，寫回時正規化（C-10）。skill 的探測器是刻意的猜測，失敗時退成 raw blob（C-03）。 |
| 3 | DLC 表才出現的欄位值，測試是否涵蓋 base 與 `dlc_01` | **沒有**：DLC 只在 E 區塊的測試裡逐列消耗過，沒有寫回、沒有走有 header 的路徑；base 的位元組 roundtrip 也只測匯出的 `out\iteminfo.pabgb`（C-02）。 |
| 4 | `..._lookup_slot_mask`（3 bytes）的 partial read | 程式碼在 E 區塊（`src/c_abi/part_prefab_dye_slot_info.rs:353-392`），不在計畫書寫的 `c_abi/iteminfo.rs`。`mask` 是固定的 `[u8; 12]`，複製 3 個位元組不會越界；doc comment、`docs/dye-editor-scope.md:614-615`、`CLAUDE.md:20` 都清楚寫了「從 2.01 起意義錯誤，請改用 `_full` / `_active_channels` / `_dye_slot_bits`」，三者一致。唯一的瑕疵是 `_full` 的「3 on 1.07-2.00」已經不可能出現（C-15）。 |
| 5 | `repr(C)` 結構的大小與對齊有沒有 `const` assert | `CrimsonItemInfoSummary` 有大小與對齊的 `const` assert，但沒有 offset 檢查，也沒有文件（C-13）。三個 handle 結構標了 `#[repr(C)]` 但裡面是 `HashMap`，對外不透明，這個屬性沒有作用（無害）。 |

## 跨區塊關注點對照

| 主題 | 結論 |
|---|---|
| 不可信輸入 | skill：全部有上限（專章）。iteminfo：有 header 的路徑以列長為上限，body-only 路徑有 A-11 的放大（C-07）。stringinfo：`slen` 先比對剩餘長度，`Vec` 用 `push` 自然成長。`parse_named_header` 的 count 以 `hdr.len() / (key_width + 12)` 為上限。 |
| Panic 面與 abort | 所有 C 範圍的 `extern "C"` 入口都有 `catch_unwind`，`Cargo.toml` 沒有 `panic = "abort"`。找到一個可達的 panic（C-05），會被轉成 `error::PANIC`。`catch_unwind` 擋不住的 abort 只剩配置失敗（C-07；skill 已經修掉）。算術：索引運算都在 64 位元的 `usize` 上，加上先比較後切片，沒有發現溢位。 |
| Roundtrip 對稱性 | no-op roundtrip：skill 與 stringinfo 有 live 測試，iteminfo 只測匯出檔、沒測 DLC（C-02）。修改後的往返：存在旗標與條件欄位都不對稱（C-10）；skill 的 header key 在 2.06 重建時遺失（C-09）。 |
| 版本漂移要明確失敗 | iteminfo：有 header 的路徑會檢查每列的消耗量與 key，很好；但不檢查列是否完整涵蓋 body（C-08），body-only 路徑在 2.06 上靜默回傳空名稱（C-06）。skill：buff matrix 的漂移會被 raw fallback 靜默吸收（C-03），post-buff 的漂移會讓整張表失敗（C-04，屬於明確失敗）。stringinfo：有 header 時逐列比對，只看 body 時依賴啟發式判斷（C-11）。 |
| Feature gate | 三個模組都沒有 `cfg` 限制（`src/lib.rs:55`、`:79`、`:84`），四種 feature 組合都會編譯，但有無條件的 `allow(dead_code)`（C-17）。本回合沒有實際編譯四種組合。 |
| `docs/c-abi-surface.md` 一致性 | 不一致（C-14、C-13）。 |

---

## 未能驗證（建議下一步）

本回合只做靜態審查，下列 plausible 項目需要在有 live install 的機器上驗證：

1. **C-01：** 以預設執行緒數連跑 `cargo test --lib --features c_abi` 三次以上，並記錄 commit charge 的峰值，確認 `--test-threads=4` 是否還有必要。
2. **C-02：** 寫一個暫時的測試（不要提交），從 live install 讀 base 與 `dlc_01` 的 iteminfo，用 `parse_iteminfo_with_header` 解析後寫回，與原始位元組比對。這同時能驗證 C-10 的情況在真實資料裡是否存在。
3. **C-03：** 記錄目前 live 2.06 的 `raw fallback=` 數字，作為之後釘住的基準值。
4. **C-05、C-08、C-09、C-11：** 用文中的構造輸入各跑一次。
5. **C-07、C-19：** 用 `size_of` 實際量出大型元素的大小；掃描 2.06 iteminfo 有沒有非 UTF-8 字串。

---

## 待查事項與假設的排除說明

### 計畫書列出的待查事項

- **(C) 「`TailSizeCache = HashMap<u8, usize>`（`buff_data.rs:34`）的 key 只有一個 `u8`，如果不同表或不同格式共用同一個 cache，tail size 可能被錯誤沿用；可能和多 GB 配置有關」 → 排除。**
  - 不共用：快取只在 `mod.rs:102`、`:140` 以區域變數建立，每次 parse 一個，沒有 `static` 或 `thread_local`，也不會離開 `skill_info`。
  - 不跨格式：`SkillFormat` 在建立快取之前就決定了（`mod.rs:138`），而且一次 parse 只有一種格式。
  - 和多 GB 配置無關：配置來自未檢查的 list count（`a9d7012` 已修正），快取的值（tail size）只拿來做 `p + tail_size > body_end` 的比較和切片（`mod.rs:410-419`），從來不拿來配置；切片長度最多就是 body 長度。
  - 在**同一張表**裡，以 `u8` 作為 key 確實假設了「同一個 `type_id` 的 tail 長度都相同」。如果這個假設不成立，後果是那一列退成 raw fallback（位元組保住，語意遺失），而不是配置錯誤；這歸入 C-03。

### 本回合查過、沒有問題的其他假設

- **skill / stringinfo / iteminfo 的 C ABI 是否都擋住了 panic：** 這三個檔案的每個 `extern "C"` 入口（不含 `free`）都包了 `catch_unwind`，指標與長度都先檢查 null；`free` 接受 null。沒有發現 `from_raw_parts` 的長度來自不可信資料。
- **`lookup_slot_mask` 的 3 位元組複製是否會越界：** `mask: [u8; 12]` 是固定長度陣列，`copy_nonoverlapping(…, 3)` 安全。
- **`CrimsonItemInfoSummary` 有沒有 build-time 的大小檢查：** 有（`iteminfo.rs:209-222`）；缺的是 offset 檢查（C-13）。
- **skill 的 `level_count` / `buff_count` 有沒有上限：** 有（100 / 200，`mod.rs:367`、`:376`）。A 區塊報告引用的 `mod.rs:373,382` 的 `with_capacity` 因此有上限。
- **StringInfo `parse_pair` 在 2.06 上是否驗證 header：** 有。它會拒絕有名稱的列（`string_info/pabgh.rs:59-64`），並逐列比對 offset 與 hash（`mod.rs:212-236`）；header 的 `size` 欄位不直接比對，但 `parse_named_header` 要求 header 剛好被消耗完，而且 offset 逐列比對過，size 與 offset 鏈之間的一致性屬於 B 區塊（`rows_are_packed`）。
- **iteminfo 有 header 的路徑是否比對 key：** 有（`item.rs:710-715`），也檢查每列的消耗量（`:704-709`）。
- **舊的 iteminfo 待查線索「`SubItem` 新 tag 是否被靜默吞掉」：** 不會，未知 tag 一律回 `InvalidData`（`structs.rs:632-637`）。
