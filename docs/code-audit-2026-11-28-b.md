# Code audit — B. Static-info 表頭／列頭佈局層（2026-11-28）

依 [`code-audit-2026-11-28-plan.md`](code-audit-2026-11-28-plan.md) 的 B 區塊做靜態審計。**沒有修改任何程式碼**，沒有執行 crimson-rs 的任何程式或測試，也沒有碰 PAPGT trailer。

- **基準：** `dev` @ `e465942`。下文的行號都以這個 commit 為準。
- **範圍：** `src/binary/{static_header,row_head,gamedata_layout}.rs`、`src/skill_info/pabgh.rs`、`src/string_info/pabgh.rs`、`src/c_abi/mod.rs:77-808`、`scripts/gamedata_layout.py`。為了判斷錯誤會傳到哪裡，也追了直接的呼叫端：九個 `*_load_from_bytes_with_header` bridge、`impl_name_only_bridge!` 產生的 bridge、`item_info/item.rs:689`、`string_info/mod.rs`、`skill_info/mod.rs`，以及 Python 端另外兩份 2.06 header 讀取器（`scripts/dump_gamedata_keys.py`、`scripts/export_for_ce.py`）。`c_abi/mod.rs:581-807` 的 `CrimsonSaveHandle` 等存檔結構屬於 G 區塊，這裡不審。
- **方法：** 逐行閱讀原始碼，用 `git blame` 判定來源歸屬，並對照 `docs/validated-patches.md`、`docs/c-abi-surface.md`、`scripts/CLAUDE.md`。
- **這次沒做到的資料比對：** 計畫的審計重點 3 要拿 `gamedata-bin/` 裡保留的 2.05.01 binary 比對。這台機器的封存在 `X:\Crimson Desert\gamedata-bin`，但不在本 session 允許存取的目錄內（存取被拒），而 live install 的檔案要經過 PAZ 解密才能讀，等於要執行 crimson-rs 本身，超出「僅靜態審查」的限制。所以審計重點 3 列在「未能驗證」，沒有寫成 finding。

### 確信度與來源歸屬的定義

- **confirmed**：只用原始碼或文件文字就能確定的事實，例如「沒有這組測試」、「列表少了某一項」、「文件和程式碼不一致」。
- **plausible**：行為上的後果（例如「會回 OK 且 0 列」）是靠追程式路徑推出來的，**沒有用位元組或實際重現驗證過**。依 CLAUDE.md 與計畫書的規定，這類一律標 plausible，即使程式路徑本身是確定性的。
- **本次引入**：引入的 commit 在 2.06 移植週期內，也就是 `95fa25f`（feat: support Crimson Desert 2.06，2026-11-27）到基準 `e465942` 之間，包括 `4f27d99`（DLC merge）。
- **既有問題**：引入時間早於 `95fa25f`。

---

## 摘要

| # | 標題 | 嚴重度 | 確信度 | 來源 |
|---|---|---|---|---|
| B-01 | `legacy_pair` / `to_legacy_layout` 失敗時靜默退回原始位元組，版本漂移不會明確失敗 | medium | plausible | 本次引入 |
| B-02 | C ABI 的 `_with_header`、`_load_from_file`、name-only 巨集與 DLC merge 在 header 無效時仍回 `OK`（0 列或雜訊列） | medium | plausible | 本次引入（巨集部分既有） |
| B-03 | test-only `gamedata_layout::resolved()` 找不到已知佈局時默默選最新佈局，live 測試會「skip 成 pass」；Python 版會丟 `LookupError` | low | plausible | 既有問題 |
| B-04 | 寬度組合與錯誤路徑的單元測試覆蓋不足；`WIDTHS` 只有 15 種組合中的 12 種 | low | confirmed | 本次引入 |
| B-05 | 直接呼叫 `parse_named_header` 的 iteminfo 與 stringinfo 不驗證 offset+size 鏈；stringinfo 從不比對 `size` | low | plausible | 本次引入 |
| B-06 | 2.06 header 的 key 和 body 列首的 key 沒有交叉比對；skill 寫回時以 body key 為準 | low | plausible | 本次引入 |
| B-07 | Python 端兩份 2.06 header 讀取器支援的寬度與檢查都和 Rust `WIDTHS` 不同步 | low | confirmed（差異）／plausible（後果） | 本次引入 |
| B-08 | 寬度一律自動偵測，不用呼叫端已知的寬度；第一個吻合的組合勝出，沒有歧義檢查 | low | plausible | 本次引入 |
| B-09 | `from_legacy_layout` 沒檢查 `count_width` 的上限（>8 會 panic），而且找列尾是 O(n²) | nit | plausible | 本次引入 |
| B-10 | `EntrySources::next_merge` 在 255 飽和，失敗的 merge 也會用掉一個來源編號 | nit | plausible | 本次引入 |
| B-11 | `impl_name_only_bridge!` 的小問題：`#[repr(C)]` 標在含 `HashMap` 的 handle 上、沒走 `slice_from_raw_or_empty`、名稱可以內含 NUL | nit | confirmed | 既有問題（NUL 部分本次引入） |
| B-12 | 文件與程式碼脫節：CLAUDE.md 的 skill `BString` 規則、`row_head.rs` 的 key 寬度說明、`c-abi-surface.md` 寫的「finds no rows」、`string_info/pabgh.rs` 整檔 `allow(dead_code)`、32 位元溢位 | nit | confirmed（文件）／plausible（行為） | 混合 |

沒有 critical 或 high 的 finding。最值得處理的是 B-01 和 B-02，這兩條同一個根因：相容層把「這不是 2.06 header」和「這是壞掉的、或寬度不認得的 2.06 header」當成同一件事，結果都走「原樣放行」的分支。

---

## Findings

### B-01 — `legacy_pair` / `to_legacy_layout` 失敗時靜默退回原始位元組

- **位置：** `src/binary/static_header.rs:228-237`（`legacy_pair`）、`:146-154`（`to_legacy_layout` 的 `?` 退出點）、`:93-101`（`detect_named_header`）。受影響的呼叫端：
  - 走 `legacy_pair` 的表解析器（22 處），例如 `src/house_info/mod.rs:31-39`、`src/faction_node_info/mod.rs:101-105`、`src/part_prefab_dye_slot_info/mod.rs:425-429`。
  - 走 `to_legacy_layout` 再 `match ... None => 掃原始 body` 的 body-only 掃描器：`src/gimmick_info/mod.rs:59-64`、`src/mission_info/mod.rs:46-51`、`src/knowledge_info/mod.rs:54`、`src/quest_gauge_info/mod.rs:42`、`src/quest_info/mod.rs:88`、`src/stage_info/mod.rs:96`、`src/sub_level_info/mod.rs:46`，以及 `src/character_info/mod.rs:121-125`（`detect_named_header` 失敗就退回 `parse_character_info_lossy`）。
- **問題與根因：** `to_legacy_layout` 有五種情況會回 `None`：
  1. header 不是 2.06 形狀；
  2. 寬度組合不在 `WIDTHS` 裡；
  3. offset+size 鏈和 `body.len()` 對不上（`rows_are_packed`）；
  4. 某列短於 `key_width + 4`；
  5. header 被截斷或多了位元組。

  `legacy_pair` 把這五種一律當成「舊版 install，不用幫忙」，原樣交出 `(body, header)`（`:235`）。可是 2.06 是唯一要支援的最新版，第 2 到第 5 種實際上代表「2.06 資料壞了」或「下一個 patch 又改了佈局」。下游的 lossy 解析器拿到 2.06 header 後，會用舊版 `count + (key, u32 offset)` 的形狀去讀，長度對不上就回 `Vec::new()`（例如 `house_info/mod.rs:37-38`、`faction_node_info/mod.rs:103-105` 的 `parse_pabgh` 錯誤被吞掉）。body-only 掃描器則去掃 2.06 body，而那裡已經沒有名稱。整條路徑上沒有任何錯誤回報。只有三個解析器會明確失敗：skill（`SkillData::parse_legacy` → `parse_pabgh` 回 `Err`，`skill_info/mod.rs:130,135`）、iteminfo（`item.rs:694-695` 回 `InvalidData`）和 stringinfo（`string_info/pabgh.rs:93-99`）。
- **可重現的情境：**
  - 拿 2.06 的 `houseinfo.staticinfobody` 和 `.staticinfoheader`，把 body 末尾多接 1 個位元組（或把 header 截掉最後 1 個位元組），呼叫 `parse_house_info_lossy(body, header)`。預期：`rows_are_packed` 不成立 → `legacy_pair` 放行 → `pabgh.len() != 2 + count*6` → 回空 `Vec`，沒有錯誤。
  - 下一個 patch 把某張表的 header 改成 `(4,12)` 或 `(1,8)`（目前 `WIDTHS` 沒有這三種組合，見 B-04），效果相同。
  - 比照計畫書「跨區塊關注點／版本漂移」那一列：相容層應該**明確失敗**，現在是悄悄回空。
- **緩解因素：** 每張表都有 live-install 測試，列數或 `KNOWN` pin 會在維護者機器上抓到問題。但這些測試在 CI 上 skip，而且 skip 會被算成 pass（I 區塊審計重點 1）。所以正式 build 的使用者（CrimsonAtomtic）只會看到「表是空的」。
- **嚴重度：** medium。不會寫壞資料，但一旦版本漂移，下游拿到的是看起來成功的空結果，CLAUDE.md「Verify a root cause」一節的教訓就是這種情況最難診斷。
- **確信度：** plausible（程式路徑已追過，沒有用位元組重現）。
- **來源：** 本次引入（`95fa25f`）。
- **建議方向（不在本次修改）：** 讓 `to_legacy_layout` 分成三種結果：「確定不是 2.06」（例如 header 長度剛好是 `cw + n*(kw+4)`）、「是 2.06」、「像 2.06 但驗證失敗」。最後一種要往上回報。

### B-02 — C ABI 在 header 無效時仍回 `OK`

- **位置：**
  - `src/c_abi/mod.rs:83-91`（`sibling_header`）
  - `:393-407`、`:442-444`、`:480-482`（`impl_name_only_bridge!` 的 `from_bytes` 和兩個 loader）
  - `:292-317`（`merge_dlc_ffi`）
  - 呼叫端範例：`src/c_abi/gimmick_info.rs:98-101`（file loader）、`:130`（body-only）、`:171`（`_with_header`）、`src/c_abi/character_info.rs:76-80`、`:303-306`
- **問題與根因：**
  1. **`_with_header` 系列**（mission、character、knowledge、stage、gimmick、quest、quest gauge、sub-level）的 `from_bytes` 不會失敗。header 無效時（B-01 的五種情況，或 `header_len == 0`）就退回掃 2.06 body，最後回 `error::OK`，handle 內 0 列，或者是雜訊列（見第 4 點）。
  2. **`*_load_from_file`** 透過 `sibling_header` 找 header。這個函式有兩個會讓 header「消失」卻不報錯的地方：(a) 副檔名比對區分大小寫（`:85-89`），Windows 上 `X.STATICINFOBODY` 或 `.PABGB` 會得到 `None`；(b) `std::fs::read(...).ok()`（`:90`）把權限錯誤、共用違規等 I/O 錯誤也吞掉了。兩種情況都會在 2.06 上得到「OK + 0 列」。
  3. **name-only 巨集**的 `$load_file` / `$load_bytes` 在 `from_bytes` 之後一律回 `OK`（`:443-444`、`:481-482`）。
  4. **`merge_dlc_ffi`** 在 DLC pair 偵測失敗時，`merge` 閉包回 0，最後回 `OK` 且 `*out_added = 0`（`:312-314`）。呼叫端無法分辨「DLC 沒有新 key」和「DLC 檔讀不懂」。
  5. 關於「雜訊列」：`c-abi-surface.md:56` 寫「the old body-only `*_load_from_bytes` finds no rows on a 2.06 body」。gimmick 掃描器要求 key 後面接 `0x00` + `/` 開頭的路徑（`gimmick_info/mod.rs:130`），誤判機率低；但 mission 掃描器只要求名稱含 `_`、key 的高位元組為 0、key 後接 `0x00`（`mission_info/mod.rs:110-125`）。2.06 body 裡還是有內嵌的 `[len][name][u32]` 子結構（gimmick 模組文件提過 2.04 有 22,484 個候選 anchor，真正的列只有 14,070 個），所以「0 列」是否成立要看資料，不是程式碼保證的。
- **可重現的情境：**
  - C# 端仍呼叫 2.06 之前的 `crimson_missioninfo_load_from_bytes(body)`，或呼叫 `crimson_missioninfo_load_from_bytes_with_header(body, NULL, 0)`。預期回 `OK`，`entry_count` 是 0 或少量雜訊。
  - `crimson_gimmickinfo_load_from_file("...\\GIMMICKINFO.STATICINFOBODY")`（大寫副檔名）。預期 `sibling_header` 回 `None` → `OK` + 0 列。
  - `crimson_knowledgeinfo_merge_dlc_from_bytes(h, dlc_body, len, truncated_header, len-1, &added)`。預期回 `OK`、`added == 0`。
- **嚴重度：** medium（和 B-01 同一個根因，在 FFI 邊界上更難察覺，因為 C# 端只看得到回傳碼）。
- **確信度：** plausible。
- **來源：** `sibling_header`、`_with_header` 和 body-only 退回邏輯是本次引入（`95fa25f`）；`merge_dlc_ffi` 是本次引入（`4f27d99`）；巨集固定回 `OK` 是既有問題（`a00be01`，2026-05-16）。
- **建議方向：** 加一個新的錯誤碼，或沿用 `BODY_PARSE`，用在「給了 header 但不是有效的 2.06 header」。在 `sibling_header` 改用不分大小寫的比對，並把 `NotFound` 以外的 I/O 錯誤往上回報。

### B-03 — test-only `gamedata_layout::resolved()` 會默默退回最新佈局

- **位置：** `src/binary/gamedata_layout.rs:56-71`（特別是 `:59-64` 的兩個 `return 0` 和 `:68` 的 `unwrap_or(0)`）；對照 `scripts/gamedata_layout.py:87-110`。
- **問題與根因：** Rust 版有三種情況都會選 `LAYOUTS[0]`：讀不到 `0008/0.pamt`、PAMT 解析失敗、PAMT 裡沒有任何已知目錄。之後 `extract_bin_from`（`:106`）找不到目錄就回 `None`，各個 live 測試把它當成「沒有安裝」，`eprintln!("skipping ...")` 後直接 `return`，結果算 pass。Python 版在同樣情況下會丟 `LookupError`，並提示要新增 `BIN_LAYOUTS`（`:105-110`）。另外 `OnceLock` 在整個測試行程只解析一次（`:57`），DLC 樹沿用 base install 的結果（`:98-106`）。
- **可重現的情境：** 假設下一個 patch 把 `0008` 的表搬到 `gamedata/foo__/bin`（2.01 就發生過一次），或改了 PAMT 格式讓 `PackMeta::parse` 失敗（2.04 改過 PAMT trie）。這時 `cargo test --lib --features c_abi` 的所有 gamedata live 測試都會 skip，整體顯示綠燈。
- **緩解因素：** runbook 第 1 步的 `export_for_ce.py` 走 Python resolver，會先大聲失敗。
- **嚴重度：** low（只影響測試，而且有 Python 端可以補救）。
- **確信度：** plausible。
- **來源：** 既有問題（`3d2a72d`，2.01 移植）。
- **建議方向：** 只有「PAMT 檔不存在」時才當成沒有安裝；PAMT 存在但沒有已知目錄時應該 `panic!`，和 Python 版的行為對齊。

### B-04 — 寬度組合與錯誤路徑的單元測試覆蓋不足

- **位置：** `src/binary/static_header.rs:239-309`（tests）、`:75-88`（`WIDTHS`）、`src/binary/row_head.rs:38-66`、`src/string_info/pabgh.rs:160-202`、`src/skill_info/pabgh.rs:106-148`。
- **問題（對應審計重點 1）：**
  - `static_header` 的 4 個單元測試只用到 `(count, key) = (2,2)`（legacy 重建、偵測）和 `(2,4)`（解析、長短 header），`read_body_row` 只測了 `kw=4`。`count_width` 1 和 4、`key_width` 1、8、12 都**沒有**單元測試。
  - `validated-patches.md` 的 2.06 列記錄了 135 張表、count 寬度 1/2/4、key 寬度 1/2/4/8/12，總共 15 種組合，但 `WIDTHS` 只列了 12 種，少了 `(1,8)`、`(1,12)`、`(4,12)`。註解寫「The count and key widths 2.06 uses」，表示這是從資料歸納出來的，本次無法驗證是否完整（見「未能驗證」）。不在清單裡的組合會掉進 B-01 的靜默退回。
  - 錯誤路徑都沒有測試：`count` 上限（`:54`）、rows 不連續或重疊（`rows_are_packed` 回 false）、某列短於 `kw+4`（`:153-154`）、`from_legacy_layout` 的 header 長度不符或 `indices.len() != count`（`:188`）、重複 offset、offset 不依序排列。
  - `row_head` 只測了 `kw` 1/2/4；`kw` 8/12 會被截成 u32（見 B-12），沒有測試。
  - `string_info/pabgh.rs` 的 2.06 named 路徑（`:57-71`，包括「列名必須為空」的拒絕分支 `:59-63`）沒有離線單元測試，只能靠 live 測試（`string_info/mod.rs:569`）覆蓋。
  - `skill_info/pabgh.rs` 沒有 2.06 相關測試；2.06 的 skill roundtrip 只在 live install 上驗證。
- **可重現的情境：** 在 `static_header::tests` 加一個 `(4,12)` 的合成 header，`detect_named_header` 會回 `None`，現有測試全部照樣通過。
- **嚴重度：** low。
- **確信度：** confirmed（測試缺漏和 `WIDTHS` 少的項目，讀原始碼就能確認）。
- **來源：** 本次引入（`95fa25f`）；`row_head` 的測試範圍是既有的（`1f32ec2`，2.04）。

### B-05 — iteminfo 與 stringinfo 不驗證 offset+size 鏈

- **位置：** `src/item_info/item.rs:694-719`、`src/string_info/pabgh.rs:57-71`、`src/string_info/mod.rs:172-237`（讀取）、`:286-311`（寫回）。
- **問題（對應審計重點 2）：** 只有 `detect_named_header` 會呼叫 `rows_are_packed`，所以只有走這條路的表才驗證「連續、不重疊、最後一列剛好到 body 結尾」，不合時回 `None`（後果見 B-01）。直接呼叫 `parse_named_header` 的兩個地方沒有這個檢查：
  - **iteminfo**（`item.rs:694`，寫死 `(2,4)`）：每列都要在 body 範圍內、被完整讀完、key 和 header 相同，但**不檢查**列與列之間的空隙、重疊、同一列被列兩次，也不檢查最後一列之後還有沒有剩下的 body 位元組。
  - **stringinfo**：`parse_pair` 確認 header 的 `offset` 等於依序走 body 得到的起點、`hash` 相同、列數相同（`mod.rs:202-236`），這已經隱含了「連續」。但 header 的 **`size` 欄位從來沒有被讀出來比對**：`StringIndexEntry` 只保留 `hash` 和 `offset`（`pabgh.rs:65-68`），寫回時 `write_named_pabgh` 用 `entry_size(e)` 重新計算 size（`mod.rs:301-305`）。
- **可重現的情境：**
  - iteminfo：把 2.06 header 的 count 加 1，並在尾端複製第 0 列的 header 項目，`parse_iteminfo_with_header` 會成功並回傳兩個 key 2200。或者在 body 尾端多接任意位元組，也會成功。
  - stringinfo：把 2.06 header 最後一列的 `size` 改成任意值，`parse_pair` 成功，`write_pabgh()` 輸出的位元組和輸入不同，而且沒有任何錯誤。這違反「Binary format must roundtrip」，卻不會被偵測到。
- **嚴重度：** low（遊戲資料本身格式正確；問題在惡意或損壞的輸入不會被拒絕，以及 roundtrip 的保證可能被悄悄打破）。
- **確信度：** plausible。
- **來源：** 本次引入（`95fa25f`）。

### B-06 — header key 和 body 列首 key 沒有交叉比對

- **位置：** `src/binary/static_header.rs:151-160`（`to_legacy_layout`）、`:167-172`（`row_indices`）、`src/character_info/mod.rs:126-133`、`src/skill_info/mod.rs:199-208`。
- **問題與根因：** `to_legacy_layout` 重建的 legacy header 和 body 用的都是 **body** 列首的 key（`:153`、`:155`、`:159`），2.06 header 裡的 `r.key` 完全沒被用到，也沒有拿來比對。結果：
  1. 各個 legacy 解析器的「body key 和 PABGH key 不同就跳過這列」（例如 `faction_node_info/mod.rs:113`、`house_info/mod.rs:52`）變成恆真，2.06 上偵測不到 header 和 body 不一致。
  2. skill 寫回時，`from_legacy_layout` 把 body key 同時寫進 header 和 body（`:211-217`）。如果原始 2.06 pair 的 header key 和 body key 不同，`parse → write` 不會報錯，輸出卻和輸入不一樣。
  3. 反過來，`character_info` 的 2.06 路徑只用 header key（`r.key as u32`，`character_info/mod.rs:130`），`read_body_row` 讀出的 body key 被丟掉。兩邊各取一半，沒有一邊做驗證。iteminfo 是唯一會比對的（`item.rs:710-715`）。
  4. key 寬度 12 的表：`read_uint` 只保留低 8 個位元組（`:24`、`:38-43`），所以 `HeaderRow::key` 就算拿來比對，也只能比 8 個位元組。目前沒有寬度 8 或 12 的表走到需要比對 key 的路徑。
- **可重現的情境：** 合成一個 `(2,4)` 的 2.06 pair，header 第 0 列 key 寫 `0x10`，body 第 0 列 key 寫 `0x11`。`to_legacy_layout` 成功，legacy header 裡的 key 是 `0x11`。`SkillData::parse` 加 `write`（假設 body 其餘部分是有效的 skill 列）會輸出 key `0x11` 的 header，和輸入不同。
- **嚴重度：** low。
- **確信度：** plausible。
- **來源：** 本次引入（`95fa25f`）。

### B-07 — Python 端的 2.06 header 讀取器和 Rust 不同步

- **位置：** `scripts/dump_gamedata_keys.py:109-139`、`scripts/export_for_ce.py:353-376`；對照 `src/binary/static_header.rs:75-113`。
- **問題（對應審計重點 5，延伸到佈局查表以外的部分）：** `gamedata_layout` 的**目錄查表**兩邊一致（見排除 E-04）。但 2.06 header 有三份互相獨立的實作，規則各不相同：

  | 實作 | count 寬度 | key 寬度 | 檢查 packing | 嘗試順序 |
  |---|---|---|---|---|
  | Rust `detect_named_header` | 1/2/4 | 1/2/4/8/12（12 種組合） | 有（`rows_are_packed`） | `(2,4),(2,2),(4,4),(2,1),…` |
  | `dump_gamedata_keys.parse_pabgh_keys` | 2/4 | 4/2/1 | 沒有，只靠 `len > cw + 8n` 和舊版形狀區分 | 先 cw，再 kw：`(2,4),(2,2),(2,1),(4,4),…` |
  | `export_for_ce._read_header_rows` | 只有 2 | 只有 4 | 沒有 | — |

  如果某張表用 `count_width = 1` 或 `key_width` 8/12，`dump_gamedata_keys` 的 named 分支會失敗，然後掉到舊版形狀的判斷（`:141-155`），結果可能是 `None`，也可能剛好吻合某個舊版形狀而回錯的 key。兩邊的嘗試順序不同，遇到有歧義的 header 時可能各選一組不同的寬度。
- **現況影響：** `PABGH_TABLES` 的 30 張表都已經在 `data/gamedata-keys-2.06/` 產出快照，表示它們的寬度都落在 Python 支援的範圍內，目前沒有實際錯誤。
- **嚴重度：** low。
- **確信度：** 差異本身 confirmed（讀原始碼即可確認）；「會回錯 key」是 plausible。
- **來源：** 本次引入（`dump_gamedata_keys.py` 的 named 分支 `95fa25f`；舊版形狀分支 `29fa188` 是既有的）。

### B-08 — 寬度一律自動偵測，第一個吻合的組合勝出

- **位置：** `src/binary/static_header.rs:93-101`；呼叫端都已經知道自己的寬度，例如 `house_info`（`kw=2`，`house_info/mod.rs:36-51`）、skill（寫死 `(2,4)`，`skill_info/mod.rs:201`）、iteminfo（寫死 `(2,4)`，`item.rs:694`）。
- **問題與根因：** 每張表的 count 和 key 寬度都是固定的，對應的解析器也都寫死了自己的寬度，但 `legacy_pair` 和 `to_legacy_layout` 不接受寬度參數，而是依 `WIDTHS` 的順序試，第一個「剛好讀完 header、而且 rows packed」的組合就勝出，沒有檢查其他組合是否也吻合。歧義的機率很低：要同時剛好讀完 header、每個名稱都是 UTF-8、offset 剛好鋪滿 `body.len()`。但有一種情況比較容易出現：一張**名稱全為空**、`kw=1` 或 `kw=2` 的表，第 0 列 offset 是 0 時，用 `kw=4` 解讀會讀到 `len' = 0`（見排除 E-05 的推導）。這時只剩 packing 檢查能擋。另外 `faction_node_info` 偵測兩次（`:96` 加 `:101`），skill 也偵測兩次（`to_legacy_layout` 加 `row_indices`），每次都完整解析 header，是重複的工作（效能上的 nit）。
- **可重現的情境：** 目前沒有已知的真實表會觸發。合成一張 `(2,2)`、所有名稱都是空字串的表，再搭配特定的 offset/size，理論上可能被 `(2,4)` 先吃掉。本次沒有建構出具體的位元組。
- **嚴重度：** low。
- **確信度：** plausible。
- **來源：** 本次引入（`95fa25f`）。
- **建議方向：** 提供 `to_legacy_layout_with(body, header, cw, kw)`，讓已知寬度的解析器直接傳入；自動偵測只留給診斷工具。

### B-09 — `from_legacy_layout` 的 API 合約與複雜度

- **位置：** `src/binary/static_header.rs:180-224`，特別是 `:200`、`:202-206`；`:54`。
- **問題：**
  1. `count_width` 是呼叫端傳入的參數，沒有檢查上限。`(count as u64).to_le_bytes()[..count_width]`（`:200`）在 `count_width > 8` 時會 index 越界 panic。在那之前，`header.get(..count_width)` 只要 header 夠長就會成功，`read_uint` 也會把長度截到 8，所以擋不住。`parse_named_header` 的 `key_width + 12`（`:54`）在 `key_width` 接近 `usize::MAX` 時會溢位（debug 下 panic，release 下 wrap）。
  2. 每一列都用 `sorted.iter().find(|&&o| o > start)` 線性搜尋列尾（`:202-206`），整體是 O(n²)。目前只有 skill（2,163 列）用到，影響可以忽略；如果日後用在 stageinfo（53,313 列）這種大表，約需 1.4×10⁹ 次比較。改用 `partition_point` 就能降到 O(n log n)。
- **可重現的情境：** `from_legacy_layout(&body, &[0u8; 16], 9, 4, &[])` → panic。目前所有呼叫端都傳 `(2,4)`，所以只有日後新增呼叫端才會遇到。
- **嚴重度：** nit。模組是 `pub(crate)`，不是外部 API；C ABI 入口外層有 `catch_unwind`。
- **確信度：** plausible。
- **來源：** 本次引入（`95fa25f`）。

### B-10 — `EntrySources::next_merge` 的編號飽和

- **位置：** `src/c_abi/mod.rs:274-277`；呼叫端 `character_info.rs:304`、`faction_info.rs:266`、`faction_node_info.rs:290`、`knowledge_info.rs:293`。
- **問題：** 來源編號是 `u8`，用 `saturating_add` 遞增，所以第 255 次以後的 merge 全部共用編號 255，`*_entry_source` 無法再分辨它們。另外 `next_merge()` 在解析之前就呼叫了，一次「0 列新增」或「DLC pair 讀不懂」（B-02）的 merge 也會用掉一個編號，之後成功的 merge 拿到的編號就不再等於「第 n 個 DLC」。
- **可重現的情境：** 先呼叫一次 `crimson_knowledgeinfo_merge_dlc_from_bytes`，傳入壞掉的 header（回 `OK`、`added=0`）；再傳入正確的 DLC，新增的列 `entry_source` 是 2，不是 1。
- **嚴重度：** nit（目前只有 `dlc_01` 一個 DLC）。
- **確信度：** plausible。
- **來源：** 本次引入（`4f27d99`）。

### B-11 — `impl_name_only_bridge!` 的一致性小問題

- **位置：** `src/c_abi/mod.rs:386-390`、`:470-479`、`:491-498`，以及 `:217-233`（`write_str_to_buf`）。
- **問題：**
  1. `#[repr(C)]` 標在 `pub struct $Handle { HashMap, Vec }` 上。這個 handle 對 C 是不透明的，`repr(C)` 沒有作用，反而容易讓人以為它的 layout 對 C 有意義。
  2. 兩個 loader 自己寫 `if len == 0 { &[] } else { from_raw_parts }`。行為和 `slice_from_raw_or_empty`（`:200-206`）相同，但該函式的文件要求「All `extern "C"` entry points ... should go through this helper」，這裡沒有遵守。
  3. `$free` 沒有包 `catch_unwind`。`HashMap<u32, String>` 的 Drop 實際上不會 panic，所以只是風格不一致。
  4. 2.06 header 的名稱只檢查是否為 UTF-8（`static_header.rs:64`），內含 `\0` 也會通過。`write_str_to_buf` 原樣複製並在尾端補 NUL，C 端讀到的字串會在第一個 `\0` 截斷，但 `required` 回報的是完整長度。舊版 body 掃描器用 `is_row_name_byte` 擋掉了 NUL，2.06 走 header 的路徑沒有這層過濾。
- **可重現的情境：** 第 4 點：2.06 header 的名稱寫成 `"A\0B"`，`lookup_string_key` 回 `OK`、`required = 4`，C 字串讀出 `"A"`。
- **嚴重度：** nit。
- **確信度：** confirmed（1 到 3 是原始碼事實）；第 4 點的後果是 plausible。
- **來源：** 1 到 3 是既有問題（`a00be01`）；第 4 點是本次引入（`95fa25f`，名稱改從 header 讀取）。

### B-12 — 文件與程式碼脫節

- **位置與問題：**
  1. **`CLAUDE.md:53`（Rules）** 寫 `BString`-shape（u32 len + bytes + u8 NUL）「used by `skill.pabgb` entry names ... handled inline in `src/skill_info/mod.rs`」。但 `read_name_bytes` 和 `write_name_bytes`（`src/skill_info/mod.rs:624-645`）沒有 NUL，`src/skill_info/post_buff.rs:194-196` 也明說那個位元組是「taken for a NUL terminator」的 `unk_post_key`。skill 的部分至少從 2.04 起就不成立了（PAPGT 的部分屬 A 區塊，本次沒查）。—— confirmed，既有問題（2.04 留下的）。
  2. **`src/binary/row_head.rs:3-5`** 寫 key「1, 2 or 4 bytes」，但同樣的表有 key 寬度 8 和 12（`validated-patches.md` 的 2.06 列寫明「same count and key widths as before」）。`read_row_head` 在 `kw > 4` 時會默默把 key 截成低 4 個位元組（`:23-24`），`body_at` 仍然正確。目前沒有呼叫端傳入 `kw > 4`。—— 文件部分 confirmed；截斷後果 plausible；既有問題（`1f32ec2`）。
  3. **`docs/c-abi-surface.md:56`** 寫「the old body-only `*_load_from_bytes` finds no rows on a 2.06 body」。這只是觀察，不是保證：mission 掃描器在 2.06 body 上可能找到雜訊列（見 B-02 第 5 點）。另外 `:71-72` 寫「nine body-only `*_load_from_bytes` calls」，但其中 iteminfo 不是 body-only（沒有 header 也能解析，只是名稱為空，`:58-59` 自己也這麼寫）。—— plausible；本次引入。
  4. **`src/string_info/pabgh.rs:21`** 用整檔 `#![allow(dead_code)]`。計畫書的跨區塊關注點要求 `allow(dead_code)` 不能掩蓋真正的 dead code。這個 allow 連 full-surface（`c_abi,python`）build 也一起關掉了，範圍比 `lib.rs:12-15` 的 `cfg_attr` 更大。目前檔內的項目看起來都有用到（`parse_pabgh`、`write_pabgh`、`CountWidth` 都被 `string_info/mod.rs` 使用），但這個 allow 會讓日後真正的 dead code 不被發現。—— confirmed；既有問題。
  5. **32 位元目標的算術溢位：** `string_info/pabgh.rs:86` 的 `4 + count32 * 8`、`:76` 的 `2 + count16 * 8`（不會溢位），以及 `static_header.rs:188` 的 `count * (key_width + 4)`，在 32 位元 `usize` 上可能 wrap。例如 `count32 = 0x2000_0000`、`data.len() == 4` 時，相等判斷會成立，接著 `read_entries` 執行 `Vec::with_capacity(0x2000_0000)`，配置 4 GB 失敗會 abort，`catch_unwind` 擋不住。專案只建置 x64（MSVC / NativeAOT，CI 是 x64 Ubuntu），所以目前不會觸發。—— plausible；`string_info/pabgh.rs` 的部分是既有問題（u32 fallback 早於 2.06），`static_header.rs` 的部分是本次引入。
- **嚴重度：** nit。

---

## 審計重點逐條結論

| # | 審計重點 | 結論 |
|---|---|---|
| 1 | `count` 寬度（1/2/4）和 `key` 寬度（1/2/4/8/12）的每種組合都有測到 | **沒有。** 單元測試只覆蓋 `(2,2)` 和 `(2,4)`；`WIDTHS` 少了 `(1,8)`、`(1,12)`、`(4,12)`。見 B-04。 |
| 2 | 2.06 header 的 `offset + size` 鏈是否驗證連續、無重疊、最後一列剛好到 body 結尾；不合時回錯還是靜默截斷 | `detect_named_header` 路徑有完整驗證（`rows_are_packed`），不合時**靜默退回原樣**，不是截斷，但也不報錯（B-01、B-02）。iteminfo 和 stringinfo 直接呼叫 `parse_named_header`，沒有驗證 packing；stringinfo 完全不比對 `size`（B-05）。 |
| 3 | `legacy_pair` 重建的位元組是否和真正的 2.05.01 佈局一致 | **本次未能驗證**（見下方「未能驗證」）。文件記錄的驗證只涵蓋 iteminfo：「rebuilding every 2.06 iteminfo row in the 2.05.01 head ... parsing it with the 2.05.01 parser: all 7,068 base and 8,218 DLC rows consumed exactly」（`validated-patches.md`）。那是「重建後能被舊版解析器完整讀完」，不是「和 2.05.01 檔案位元組相同」。其他表沒有對應的紀錄。 |
| 4 | `*_load_from_bytes_with_header` 和舊的 body-only 入口在漏傳 header 時的錯誤路徑 | **沒有錯誤路徑。** 一律回 `OK` 加 0 列（或雜訊列）。iteminfo 例外：給了非 2.06 的 header 會回 `BODY_PARSE`，但 `header_len == 0` 時被當成沒給 header（`c_abi/iteminfo.rs:325`）。見 B-02。 |
| 5 | Rust 和 Python 兩份 `gamedata_layout` 查表是否同步（newest-first、fallback 順序） | **目錄查表同步**（E-04）。差異在於找不到已知佈局時：Rust 默默選最新佈局，Python 丟 `LookupError`（B-03）。2.06 header 讀取器有三份實作，彼此不同步（B-07）。 |
| 6 | DLC 表的 row index 不等於自身位置；確認沒有程式碼假設它等於位置 | **確認沒有。** 見 E-03。 |

## 跨區塊關注點逐條結論

| 主題 | 結論 |
|---|---|
| 不可信輸入的配置上限 | B 區塊的配置都有上限：`parse_named_header` 用 `count ≤ len/(kw+12)` 擋住（`static_header.rs:54`）；`to_legacy_layout` 和 `from_legacy_layout` 的配置量和輸入大小成正比；`skill_info/pabgh.rs:31-53` 和 `string_info/pabgh.rs:76-88` 先比對長度相等才配置。只有 32 位元目標有溢位問題（B-12 第 5 點）。 |
| Panic 面與 abort | 面對不可信輸入時，所有讀取都用 `get` 和 `checked_add`，沒有找到會 panic 的位置（E-01）。只有內部 API 合約沒有防護（B-09）。B 區塊在 x64 上沒有 abort 風險。 |
| Roundtrip 對稱性 | skill：`to_legacy_layout` 搭配 `row_indices` 和 `from_legacy_layout` 是對稱的，有合成測試（`static_header.rs:261-264`）和 live 測試。兩個缺口：header 和 body key 不一致時會被悄悄改寫（B-06）；stringinfo 的 `size` 欄位會被悄悄改寫（B-05）。 |
| 版本漂移明確失敗 | **不符合。** 這是本區塊最主要的問題（B-01、B-02、B-03）。 |
| Feature gate 組合 | `binary::{row_head, static_header}` 是 `pub(crate)`，所有組合都會編譯；`gamedata_layout` 是 `#[cfg(test)]`；呼叫端的表模組大多是 `#[cfg(feature = "c_abi")]`（`lib.rs:18-77`），partial build 的 dead code 由 `lib.rs:12-15` 的 `cfg_attr` 消音，full surface 下由 CI 的 strict clippy 把關。唯一例外是 `string_info/pabgh.rs:21` 的整檔 allow（B-12 第 4 點）。CI 只跑 default 和 `c_abi,python` 兩種組合（屬 I 區塊）。 |
| `docs/c-abi-surface.md` 一致性 | 9 個 `*_load_from_bytes_with_header` 符號、`sibling_header` 的說明、巨集「6 個函式」的說明，都和程式碼一致（E-06）。不一致的是「finds no rows」這個保證的措辭，以及「nine body-only」裡包含了 iteminfo（B-12 第 3 點）。 |

---

## 計畫書「掃描時注意到的待查事項」

計畫書列出的五條待查事項分別分配給 C、I、I、G、D，**沒有分配給 B 的項目**，所以本報告沒有需要結案的 B 類待查事項。

## 排除說明（查過、確認沒有問題的項目）

- **E-01 惡意 header 不會造成 panic 或 OOM：** `parse_named_header`（`static_header.rs:47-72`）的每次讀取都用 `hdr.get(..)?`，名稱長度用 `checked_add`，`count` 在配置前用最小列長 `kw+12` 限制上限。`read_body_row` 的 `&row[key_width + 4..]`（`:128`）前面已經有 `row.get(key_width..key_width+4)?` 確認長度，不會越界。`HeaderRow::range` 和 `rows_are_packed` 在 64 位元上的加總不會溢位，因為 `count` 有上限，而每列 size 最多 `u32::MAX`。
- **E-02 `rows_are_packed` 的邏輯正確：** 從 0 開始，要求每列的 `offset == 前面所有 size 的總和`，最後總和要等於 `body_len`，這完整表達了「依 header 順序、連續、不重疊、剛好鋪滿」。
- **E-03 沒有程式碼假設 row index 等於位置（審計重點 6）：** 在 `src/` 搜尋 `row_index` 和 `.index` 後，只找到以下用途，都沒有這個假設：
  - `SkillData.row_indices` 原樣保存，再原樣寫回（`skill_info/mod.rs:123-127, 199-201`）。
  - `python.rs:118` 的 lossy 掃描只要求 `index < 100_000`（DLC 的範圍是 167..8342，符合）。
  - `character_info` 的 2.06 路徑把 index 讀出來後就丟掉。
  - `item.rs:763` 的 `row_index == i` 斷言只用在 base 表（`out/iteminfo.pabgb`），不涵蓋 DLC。

  `scripts/` 也沒有使用 row index 的程式碼。
- **E-04 Rust 和 Python 的目錄查表同步：** `src/binary/gamedata_layout.rs:32-43` 的 `LAYOUTS` 和 `scripts/gamedata_layout.py:63-66` 的 `BIN_LAYOUTS`，兩個項目和順序完全相同，都是 newest-first（`binarystaticinfo__/bin` 加 `staticinfobody/staticinfoheader`，接著 `binary__/client/bin` 加 `pabgb/pabgh`）。PALOC 的 `PALOC_ROOT` 也相同，2.01 之後的「語言子目錄」判斷和 2.01 之前的 `localizationstring_<lang>.paloc` 判斷順序也一致。
- **E-05 舊版 header 被誤判成 2.06 的機率：** 同一組 `(cw, kw)` 下，舊版每列 `kw+4` 位元組，2.06 每列至少 `kw+12` 位元組。舊版 header 長度 `cw + n(kw+4)` 除以 `kw+12`，在 `n ≥ 1` 時一定小於 `n`，所以會被 `:54` 的上限擋掉。不同 cw 的交叉情況（例如 u32 count 被當成 u16）還要通過「剛好讀完 header」和 packing 兩道檢查，實際上不會發生。`n = 0` 的空表會被當成 2.06 重建，但重建的結果和輸入等價，沒有危害。stringinfo 的 named 解析也套用同一個上限（`string_info/pabgh.rs:58`）。（同 cw、不同 kw 的歧義見 B-08。）
- **E-06 C ABI 的 `_with_header` 清單和文件一致：** `sibling_header` 的呼叫端正好是 9 個（character、gimmick、iteminfo、knowledge、mission、quest gauge、quest、stage、sub-level），和 `c-abi-surface.md:53-64` 列的一一對應。巨集產生的 6 個函式和 `c-abi-surface.md:537-539` 相符。
- **E-07 skill 和 stringinfo 舊版 pabgh 的配置：** `skill_info/pabgh.rs:28-55` 先要求 `len == 2 + count*8`（u16 count，不會溢位）才配置；`string_info/pabgh.rs:73-90` 也是先比對長度相等才呼叫 `read_entries`（64 位元上安全）。
- **E-08 `faction_node_info` 的兩階段判斷不會讀錯：** 如果 `detect_named_header` 成功、但 `to_legacy_layout` 因為列太短失敗，`legacy_pair` 會原樣放行，接著 `parse_pabgh` 拿 2.06 header 失敗而回空，不會用 2.06 的 offset 去讀 2.05 的列（只有 B-01 的靜默問題）。
- **E-09 名稱的 UTF-8 驗證：** 2.06 header 的名稱（`static_header.rs:64`）、舊版 row head（`row_head.rs:21`）、`from_legacy_layout`（`:210`）都做嚴格的 UTF-8 驗證，不會把非法序列交給 `String` 或 C 端（NUL 的問題見 B-11 第 4 點）。

## 未能驗證（需要在有封存或 live install 的機器上執行）

1. **審計重點 3：** 對每一張 bridge 用到的表，拿 `to_legacy_layout(2.06)` 的每一列和 `gamedata-bin/2.05.01/` 的同 key 列比對（內容沒變的列應該位元組相同），並確認 2.05.01 header 的列順序就是 body 的 offset 順序，也就是重建後的 legacy header 是否和真正的 2.05.01 header 同序。
2. **`WIDTHS` 完整性與歧義（B-04、B-08）：** 對 2.06 的 135 張 base 表和 `dlc_01` 的表，各自計算 15 種組合中有幾種能通過「`parse_named_header` 加 `rows_are_packed`」。預期每張表剛好 1 種，而且都在 `WIDTHS` 裡。
3. **B-02 第 5 點：** 對 8 個 body-only 掃描器各餵一份 2.06 body（不給 header），記錄回傳的列數，確認「finds no rows」是否成立。
4. **計畫書的驗證項：** 把 `data/gamedata-keys-2.06/*.txt` 和實際解析出的 key 集合比對。需要執行解析器，本次沒做。
