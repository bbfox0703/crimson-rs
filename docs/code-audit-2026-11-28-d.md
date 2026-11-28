# Code audit — D. 一般 gamedata key-resolver 橋接（D1–D3，2026-11-28）

依 [`code-audit-2026-11-28-plan.md`](code-audit-2026-11-28-plan.md) 的 D 區塊做靜態審計。**沒有修改任何程式碼**，沒有執行 crimson-rs 的任何程式或測試，沒有派發 subagent，也沒有碰 PAPGT trailer。

- **基準：** `dev` @ `e465942`。下文的行號都以這個 commit 為準。
- **範圍：**
  - **D1 任務進度：** `src/{quest,quest_gauge,mission,stage,knowledge,sub_level,global_game_event,global_game_event_group,game_advice,game_advice_group}_info/mod.rs` 與對應的 `src/c_abi/*.rs`，以及策展表 `src/c_abi/{main_quest_chapter,side_quest_faction,live_titles}.rs`、`docs/ref-gamedata/{main,side}-quest-list.md`。
  - **D2 世界／陣營／角色：** `character_info`（`src/c_abi/character_info.rs:1-765`）、`faction_info`、`faction_node_info`、`faction_relation_group_info`、`faction_spawn_data_info`、`mercenary_info`、`gimmick_info`、`region_info`、`trigger_region_info`、`house_info`。
  - **D3 經濟／雜項：** `store_info`、`royal_supply_info`、`item_group_info`、`craft_tool_info`、`craft_tool_group_info`、`gameplay_variable_info`、`reserve_slot_info`。
  - 為了判斷錯誤怎麼傳出去，也讀了直接依賴：`src/c_abi/mod.rs:77-580`（`sibling_header`、`EntrySources`、`merge_dlc_ffi`、`impl_name_only_bridge!`）、`src/binary/{static_header,row_head}.rs`、`src/skill_info/pabgh.rs`、`src/c_abi/paloc.rs` 的 `lookup_str`、`.github/workflows/ci.yml`、`src/lib.rs` 的 feature gate。B 區塊已經回報的共用問題（B-01、B-02、B-10、B-11）這裡只引用，不重複展開。
- **方法：** 逐行閱讀原始碼；用 `data/gamedata-keys-2.06/*.txt`（從 2.06 實檔匯出的 key 快照）以 Grep 計數，檢查各掃描器的 key 規則會排除多少真實列；對照 `docs/c-abi-surface.md`、`docs/dev-log.md`、`docs/ref-gamedata/*.md`。
- **工具限制：** 本 session 的 Bash／git 指令需要核准，嘗試用 `python -I` 對 key 快照做集合運算（雜湊 key 的 lo24 碰撞分析）時被拒，所以那一項列在「未能驗證」。`git blame` 也無法執行，來源歸屬改用 `docs/dev-log.md` 的 PR 紀錄（#115–#127）判定。

### 確信度與來源歸屬的定義

沿用 B 區塊報告的定義：

- **confirmed**：只靠原始碼、文件或 key 快照就能確定的事實，例如「這條 key 規則排除了快照裡的 N 個 key」、「CI 沒有啟用這個 feature」、「文件與程式碼不一致」。
- **plausible**：行為後果（例如「C# 端會拿到 `NOT_FOUND`」）是追程式路徑推出來的，**沒有用位元組或實際重現驗證過**。依 CLAUDE.md 和計畫書的規定，這類一律標 plausible。
- **本次引入**：引入的 PR 落在 2.06 移植週期內，也就是 #115（`95fa25f`，2026-11-27）到基準 `e465942` 之間（#115–#127）。
- **既有問題**：引入時間早於 #115。

---

## 摘要

| # | 標題 | 嚴重度 | 確信度 | 來源 |
|---|---|---|---|---|
| D-01 | 2.06 的 `_with_header`／`_load_from_file` 路徑丟掉 header 的權威列表，改用啟發式掃描重建的 body：mission 少 2,444 個 key，stage 至少少 1,177 個，quest 少 `StartGame`，gimmick 至少少 12 個；文件還要求下游改用這條路徑 | medium | confirmed（被排除的 key）／plausible（存檔端影響） | 本次引入（設計）；掃描器規則既有 |
| D-02 | characterinfo 在 2.06 載入了 359 個雜湊 32-bit key 列，但 `lookup_string_key`／`lookup_display_name`／`resolve_portrait` 只查 lo24，和 `lookup_faction_key`、文件的「先完整比對」不一致 | medium | confirmed（程式碼）／plausible（錯誤命中） | 本次引入 |
| D-03 | CI 用 default features 跑 `cargo test --lib`，D 區塊全部模組都在 `c_abi` 之後，離線單元測試在 CI 上從未編譯、也從未執行；測試註解卻寫「runs on CI」 | medium | confirmed | 既有問題（註解部分本次以前即有） |
| D-04 | gimmick、character 的 `lookup_display_name` 不檢查 key 是否在表內，任何 u32 都直接組 PALOC key；表外 key 也可能回 `OK` 加上別張表的字串 | low | confirmed（程式碼）／plausible（碰撞） | 既有問題 |
| D-05 | knowledge／characterinfo 的 DLC merge 收到空 header 時，會用 body-only 掃描讀 2.06 body，雜訊列以 DLC 來源永久加入 handle，之後正確的 merge 也蓋不掉 | low | plausible | 本次引入 |
| D-06 | 固定偏移量的欄位抽取（faction group、factionnode knowledge、character faction、global event group）沒有任何資料內的驗證，佈局一漂移就回看起來合理的錯誤 key | low | plausible | 本次引入（faction／node／character）；既有（global event） |
| D-07 | live 測試的列數 pin 太寬（store `>=250` 但實際 441、mission `>1000` 但實際 6,872 列），而且名稱長度門檻會默默丟列，沒有「解析列數 = header 列數」的檢查 | low | confirmed | 既有問題 |
| D-08 | `globalgameevent` 的 `paloc_key` 只接受剛好 14 位數；註解說「event_key < 0x4300」，但 2.00 起就有 0x4308–0x430b | nit | plausible | 既有問題 |
| D-09 | `faction_info` 手寫版和巨集版的小差異：`get_entry` 沒先清 `*out_key`；safe fn `slice` 內含 `from_raw_parts` | nit | confirmed | 本次引入 |
| D-10 | 策展表的 drift 檢查只有單向；`live_titles` 用 last-wins 建 map，和 bridge 的 first-wins 不同 | nit | confirmed（程式碼）／plausible（後果） | 既有問題 |
| D-11 | 文件與程式碼脫節：`c-abi-surface.md` 沒有逐一列出 D 區塊的符號、列數過時（計畫書待查事項）、巨集說明、模組文件互相矛盾等十餘處 | nit | confirmed | 混合 |

沒有 critical 或 high 的 finding。最需要處理的是 D-01 和 D-02，兩者都在 2.06 移植時引入，也都直接影響 Save Editor 的 key 解析。D-03 讓 D 區塊的離線測試在 branch protection 下形同不存在，雖然根因在 I 區塊（CI 設定），對 D 的影響最大。

---

## Findings

### D-01 — 2.06 的 `_with_header` 路徑丟掉 header 的權威列表，改跑啟發式掃描

- **位置：**
  - 解析器：`src/mission_info/mod.rs:46-51`、`src/quest_info/mod.rs:87-92`、`src/stage_info/mod.rs:95-100`、`src/knowledge_info/mod.rs:53-58`、`src/gimmick_info/mod.rs:59-64`、`src/quest_gauge_info/mod.rs:41-46`、`src/sub_level_info/mod.rs:45-50`。
  - 各掃描器的 key／名稱規則：`src/mission_info/mod.rs:124-128`、`src/quest_info/mod.rs:137-141`、`src/stage_info/mod.rs:162-166`、`src/gimmick_info/mod.rs:123-130`、`src/sub_level_info/mod.rs:103-107`、`src/knowledge_info/mod.rs:104-112`。
  - C ABI：`src/c_abi/mission_info.rs:62-66`（沒有 `_with_pabgh` 路徑）、`src/c_abi/quest_info.rs:42-47`、`src/c_abi/stage_info.rs:39-44`、`src/c_abi/gimmick_info.rs:53-57`。
  - 文件：`docs/c-abi-surface.md:53-57`、`:71-74`（要求 CrimsonAtomtic 把九個 body-only 呼叫改成 `_with_header`）。
- **問題與根因：** 在 2.06，`.staticinfoheader` 已經逐列列出 `(key, name, offset, size)`，是權威的 key／名稱列表。可是這七個 `*_lossy_with_header` 函式只把 header 拿來呼叫 `to_legacy_layout`，重建 2.05.01 形狀的 body，然後**再對重建後的 body 跑 anchor scanner**。也就是說，header 已經提供的精確列表被丟掉，換回 2.04 時代為了「沒有 index」而設計的啟發式規則。這些規則在 2.06 的實際後果如下（用 2.06 key 快照 Grep 計數）：

  | 表 | 2.06 列數 | 掃描器規則 | 規則排除的 key | 有沒有精確替代路徑 |
  |---|---:|---|---:|---|
  | missioninfo | 6,872 | `(key >> 24) == 0`、名稱需含 `_` | **2,444**（`4294967294`… 即 `0xFFFFFFxx`，模組文件 `:25-28` 也說這是 `Challenge_*` 列） | **沒有**，mission bridge 只有 lossy 路徑 |
  | stageinfo | 53,313 | `(key >> 24) < 0x80`、名稱需含 `_`、名稱字元集 | **1,177**（key ≥ 2³¹，例如 `2425717811`、`3605308157`），外加含逗號的 `LevelSequencerSpawn_UUID_…` 名稱（1.08 時 1,403 列，2.06 未計） | 有：`crimson_stageinfo_load_*_with_pabgh` |
  | questinfo | 1,173 | 名稱需含 `_`、長度 3..=128 | `StartGame`（key 1）；`c_abi_questinfo_with_pabgh_recovers_no_underscore_rows`（`src/c_abi/quest_info.rs:656-723`）在 2.06 上斷言 `_with_header` 路徑回 `NOT_FOUND` | 有：`crimson_questinfo_load_*_with_pabgh` |
  | gimmickinfo | 14,218 | `(key >> 24) < 0x10`、首字母、含 `_`、後接 `/` 路徑 | **12**（`999750001`、`920070001`–`920070008`、`900110001`、`900060001`、`900070001`）；模組文件 `:21-28` 記錄 2.04 時掃描器找到 14,070 列，index 有 14,210 列，差 140 列 | **沒有** |
  | knowledgeinfo、questgaugeinfo、sublevelinfo | 6,839／511／44 | 各自的規則 | key 規則在 2.06 不排除任何列（快照中沒有超出範圍的 key）；dev-log #118 記錄 knowledge 讀到 6,839 列，和 header 一致 | 沒有 |

  另外，先找到的列會擋住後找到的（first-wins dedup，各 bridge 一致，見排除 E-01）。如果 body 雜訊出現在真實列之前，而且雜訊的 key 剛好等於後面某列的 key，就會變成雜訊勝出。`sub_level_info` 的名稱最短只有 2 個字元（`Hp`），而且不要求 `_`；模組文件 `:68-71` 和測試 `:188-190` 都承認有「body-byte collisions that share a row key」，只靠「真實列在 offset 0」來保證，但這只對第一列成立。`knowledge_info` 測試註解 `:189-195` 說「the HashMap dedupes by key, so real KnowledgeKey lookups return the correct name」，在 first-wins 下其實不保證。

  `character_info` 是唯一的反例：`parse_character_info_lossy_with_header`（`src/character_info/mod.rs:121-136`）在 2.06 直接從 header 讀出每一列，所以雜湊 key 的列也拿得到。其他七張表沒有這樣做。
- **可重現的情境：**
  - 存檔的 `_missionStateList[N]._key` 是任一 `0xFFFFFFxx` 的 `Challenge_*` MissionKey。C# 端依 `c-abi-surface.md:71-74` 用 `crimson_missioninfo_load_from_bytes_with_header` 載入，`lookup_string_key` 和 `lookup_display_name` 都會回 `NOT_FOUND`。mission 沒有其他載入路徑可以補救。
  - StageKey `2425717811`（快照第 48,101 列）：`_with_header` 載入的 handle 回 `NOT_FOUND`，`_with_pabgh` 載入的回 `OK`。兩個 loader 吃的是同一組 `(body, header)` 位元組，結果卻不同。
  - QuestKey 1（`StartGame`）：見上面那個 live 測試。
- **嚴重度：** medium。不會寫壞資料，但 Save Editor 的名稱欄會整批空白：mission 表 36% 的列查不到，而且文件引導下游走的就是這條路徑。
- **確信度：** 「這些 key 不可能被掃描器回傳」是 **confirmed**：key 規則是確定性的程式條件，被排除的 key 數直接從 2.06 key 快照計出，quest 的部分還有現成的 live 測試斷言。「存檔裡確實出現這些 key、C# 端確實看到空白」是 **plausible**，本次沒有拿存檔驗證。
- **來源：** 「有 header 卻仍然掃描」的設計是本次引入（#115）；各掃描器的 key／名稱規則是既有的。
- **建議方向（不在本次修改）：** 讓七個 `*_lossy_with_header` 在 `detect_named_header` 成功時，比照 `character_info` 直接回傳 header 的 `(key, name)`，只有在偵測失敗時才退回掃描；並為 mission、gimmick 補上 `_with_pabgh` 路徑，或在文件中把 quest／stage 的推薦路徑改成 `_with_pabgh`。

### D-02 — characterinfo 的雜湊 key 列載入了，但三個查詢函式只查 lo24

- **位置：**
  - 載入：`src/character_info/mod.rs:121-136`（2.06 header 路徑，`r.key as u32`，包含雜湊 key）、`:22-26`（359 列是雜湊 32-bit key，例如 `NHM_Noble_1` = `0x77395fed`）。
  - 查詢：`src/c_abi/character_info.rs:266-269`（`lookup_string_key`：只查 `character_key & 0x00FF_FFFF`）、`:443-446`（`lookup_display_name`：PALOC hi32 一律用 lo24）、`:565`、`:572`（`resolve_portrait`：同上）。
  - 對照：`:354-357`（`lookup_faction_key`：先完整比對，再查 lo24）；`:332-334` 的文件寫「matched as given first …, as [`crimson_characterinfo_lookup_string_key`] does」。
  - 文件：`docs/c-abi-surface.md:84-85`（「Matches the key as given, then with the save cat-byte stripped」）、`:118-120`（「the 359 rows with a hashed 32-bit key are included too」）。
- **問題與根因：** #115 讓 2.06 的 characterinfo 從 header 讀出所有列，包括 359 個 hi-byte 不為 0 的雜湊 key（2.06 快照中 key ≥ 2²⁴ 的正好是 359 個）。`get_entry` 會原樣列舉這些完整 key，`lookup_faction_key` 也能用完整 key 查到。但 `lookup_string_key` 會先剝掉 hi8 再查，所以 `lookup_string_key(0x77395fed)` 實際查的是 `0x395fed`：
  1. 該 lo24 不在表內時，回 `NOT_FOUND`。同一個 key，`get_entry` 列得出來、`lookup_faction_key` 也查得到，`lookup_string_key` 卻查不到。
  2. 該 lo24 剛好是另一列的 key 時，回 `OK` 和**另一個角色**的名稱。
  3. `lookup_display_name` 和 `resolve_portrait` 也用 lo24 組 PALOC key，雜湊 key 列的顯示名稱和肖像比對都會落在錯誤的 hi32 上。

  `lookup_faction_key` 的文件說它的比對方式「和 `lookup_string_key` 一樣」，這句話不成立。
- **可重現的情境：** 在 2.06 上用 `crimson_characterinfo_load_from_bytes_with_header` 載入，對每個 `get_entry` 回傳的 key `k` 呼叫 `lookup_string_key(k)`。預期所有 hi8 不為 0 的 `k`（359 個）都不會回傳 `get_entry` 給的那個名稱：要嘛 `NOT_FOUND`，要嘛是 lo24 碰撞到的另一列名稱。
- **嚴重度：** medium。影響範圍 359／7,315 列（約 5%），但結果可能是「錯的名稱」而不只是「查不到」。在 Save Editor 裡，錯誤的角色名稱比空白更容易誤導使用者。
- **確信度：** 不一致本身是 **confirmed**（讀原始碼即可確定）。「有多少個雜湊 key 的 lo24 會碰撞到真實列」是 **plausible**，本次無法執行集合運算（見「未能驗證」）。
- **來源：** 本次引入。雜湊 key 列從 #115 開始載入，`lookup_faction_key` 和那句文件是 #117 加的；`lookup_string_key` 的 lo24 剝除規則是既有的，在 #115 之前只會遇到 hi8 = 0 的列，所以不會出錯。
- **建議方向：** 三個查詢函式都改成和 `lookup_faction_key` 相同的「先完整比對、再查 lo24」；`lookup_display_name` 在完整 key 命中時，PALOC hi32 也應該用完整 key（需要先用 PALOC 驗證雜湊 key 列的實際 hi32）。

### D-03 — CI 從未編譯、也從未執行 D 區塊的任何測試

- **位置：** `.github/workflows/ci.yml:57-58`（`cargo test --lib`，沒有 `--features`）；`src/lib.rs:18-93`（D 區塊的每個 `*_info` 模組和整個 `c_abi` 都是 `#[cfg(feature = "c_abi")]`）；`src/c_abi/main_quest_chapter.rs:869-872`（測試模組文件：「That one skips cleanly without an install, so everything here runs on CI」）；`src/c_abi/live_titles.rs:6-7`（「the tests built on it skip cleanly on CI」）。
- **問題與根因：** CI 的測試步驟只用 default features，D 區塊的程式碼完全不會被編進測試二進位檔。clippy 步驟用了 `--all-targets --features c_abi,python`，所以測試程式碼會被**編譯檢查**，但不會**執行**。結果是：
  - 不依賴 live install、本來可以在 CI 上把關的純邏輯測試**從未在 CI 上執行過**。例如 `curated_table_integrity`、`entry_keys_are_unique`、`unresolved_rows_are_the_known_five`、`collision_first_match_is_table_order`（main／side quest）、`extract_paloc_key_round_trip`／`_rejects_*`（global game event）、各 bridge 的 `*_null_args`、`*_empty_bytes_yields_empty_handle`、`*_load_bad_path_returns_io`、`c_abi_factionnode_dlc_merge_args`。
  - 策展表測試的註解明說「everything here runs on CI」，事實不然。
  - 這和 I 區塊審計重點 1（skip 被算成 pass）是兩回事：這裡是**連 skip 都沒有發生**。
- **可重現的情境：** 在 `main_quest_chapter.rs` 的 `ROWS` 裡複製一列，讓 `entry_keys_are_unique` 失敗，然後開 PR。CI 的 `cargo test --lib` 照樣綠燈，`main` 的 branch protection 放行。
- **嚴重度：** medium。CI 是 `main` 唯一的必要檢查，而 D 區塊的離線回歸防線在 CI 上等於不存在，只能靠維護者在本機用 `--features c_abi` 跑。
- **確信度：** confirmed（`ci.yml` 和 `lib.rs` 的原始碼事實）。
- **來源：** 既有問題。CI 設定和 feature gate 都早於 #115；「runs on CI」的註解也早於 2.06 週期（策展表的 key 化在 2.02／2.03 時期）。
- **建議方向：** 屬 I 區塊的修正範圍。最小改動是在 `ci.yml` 加一步 `cargo test --lib --features c_abi -- --test-threads=4`（live 測試會 skip）。

### D-04 — gimmick、character 的 `lookup_display_name` 不檢查 key 是否在表內

- **位置：** `src/c_abi/gimmick_info.rs:256-262`、`:301-315`；`src/c_abi/character_info.rs:409-412`、`:443-450`。對照 `src/faction_info/mod.rs:40-45`（文件自己記錄過：FactionKey 拿去查 `0x30` 會得到 character.paloc 的角色名稱）、`src/c_abi/character_info.rs:4325-4331`（probe 列出的 namespace 對照：`0x30` CharacterKey、`0x70` ItemKey、`0x200` GimmickInfoKey）。
- **問題與根因：** 這兩個「no hash hop」的 bridge 刻意不查 handle，直接把呼叫端給的 u32 組成 `(key << 32) | lo32`（character 會先剝掉 hi8）去查 PALOC。所以：
  1. 不在 gimmickinfo／characterinfo 裡的 key（例如被 D-01 掃描器漏掉的 12 個 gimmick key 以外的任意值、呼叫端傳錯欄位、或 FactionKey 誤傳進 character bridge），只要 PALOC 剛好有 `(key << 32) | lo32` 這個 entry，就會回 `OK`。
  2. faction 模組文件記錄的正是這種「key 空間重疊、在錯誤 namespace 上命中」的實例（FactionKey 1000000 → "Hernandian Guard"）。character 的 display name 對任何非 CharacterKey 都有同樣的風險，回傳碼卻無法區分。
  3. 呼叫端拿到的 `OK` 不代表「這是一個存在的 gimmick／character」，和 `lookup_string_key` 的 `NOT_FOUND` 語意不一致。
- **可重現的情境：** 用 `crimson_characterinfo_lookup_display_name(h, paloc, 1_000_000 /* FactionKey */, 0x30, …)`：預期回 `OK`，內容是 "Hernandian Guard"（faction 模組文件 `:41-44` 的實例）。用一個不在 gimmickinfo 裡、但 `(k << 32) | 0x200` 存在於 PALOC 的 `k` 呼叫 gimmick 版本，也會回 `OK`。
- **嚴重度：** low。用法正確（key 確實來自該欄位）時不會出錯；問題在於 API 不擋誤用，也不擋 key 不存在的情況。
- **確信度：** 「不檢查 handle」是 confirmed（程式碼與文件都明說）；「會命中錯誤字串」是 plausible（faction 的例子是 faction bridge 的文件記錄，本次沒有重新對 PALOC 驗證）。
- **來源：** 既有問題（兩個 bridge 都早於 #115）。
- **建議方向：** 先用 `by_key` 檢查 key 是否存在（character 用 D-02 的兩段比對），不存在就回 `NOT_FOUND`。如果需要保留「不在表內也查 PALOC」的行為，就另開一個明確命名的函式。

### D-05 — DLC merge 收到空 header 時，雜訊列會永久加入 handle

- **位置：** `src/c_abi/knowledge_info.rs:291-295`、`src/c_abi/character_info.rs:303-306`（`(!hd.is_empty()).then_some(hd)`：空 header 走 body-only 掃描）；`src/c_abi/mod.rs:292-317`（`merge_dlc_ffi` 一律回 `OK`）；`docs/c-abi-surface.md:207-209`（「header may be empty (null, 0) only for knowledge / characterinfo on a pre-2.06 body」）。
- **問題與根因：** B-02 已經指出 merge 在 header 無效時回 `OK` 加上 `added = 0`。這裡補充 D 區塊特有、而且更糟的後果：knowledge 和 characterinfo 在 `header_len == 0` 時，會用 body-only 掃描器讀 DLC body。2.06 body 已經沒有列名，但 body 裡仍有內嵌的 `[len][name][u32]` 子結構（B-02 第 5 點、`gimmick_info` 模組文件 `:21-28`）。knowledge 的規則只要求首字母、含 `_`、`hi8 < 0x80`，所以雜訊列可能被當成「DLC 新增的列」，以 `source = 1` 加進 handle。由於 `absorb` 是 first-wins，而且沒有任何撤銷 API：
  1. 雜訊列會出現在 `get_entry` 列舉和 `entry_count` 裡，`entry_source` 回報為 DLC。
  2. 如果雜訊的 key 剛好等於某個真實 DLC key，之後呼叫端改用正確 header 重新 merge 時，那個真實列會被擋掉（`added` 少算），而且呼叫端無從察覺。
  3. 依 B-10，第二次 merge 拿到的來源編號是 2，不是 1。
- **可重現的情境：** 依序呼叫 `crimson_knowledgeinfo_merge_dlc_from_bytes(h, dlc_body, len, NULL, 0, &a)`，再呼叫 `crimson_knowledgeinfo_merge_dlc_from_bytes(h, dlc_body, len, dlc_header, hlen, &b)`。預期 `a` 不一定是 0，而 `b` 會比單獨做一次正確 merge 時的 795 少 `a` 中 key 重疊的部分。
- **嚴重度：** low。文件已經規定 2.06 必須傳 header，所以只有呼叫端誤用時才會發生；但誤用後無法復原，handle 必須整個重建。
- **確信度：** plausible。本次沒有實際餵入 2.06 DLC body，也沒有計算雜訊列數。
- **來源：** 本次引入（#119）。
- **建議方向：** 如果 body 和 2.06 header 形狀不符，或者 header 為空而 body 看起來是 2.06（列首是 `[key][u32 index]`），就回錯誤碼且不改動 handle。也可以比照 `item_part_prefab` 的 merge，先完整解析、確認成功後再寫入 handle（dev-log #124 記錄那邊採用這種原子做法）。

### D-06 — 固定偏移量的欄位抽取沒有資料內的驗證

- **位置：**
  - `src/faction_info/mod.rs:99-110`：`faction_group_key`、`relation_group_key` 固定讀 `body_at + 3`／`+ 5`；模組文件 `:18` 說前面的 `[u8; 3]` 「zero on every row」，但程式沒有檢查。讀不到時 `unwrap_or(0)`，和「沒有 group」無法區分。
  - `src/faction_node_info/mod.rs:95-100`、`:116-120`：KnowledgeKey 的位置（8 或 11）只看 header 是不是 2.06 形狀來決定。
  - `src/character_info/mod.rs:111-115`、`:132`、`:153`：FactionKey 固定在 151（2.06）／149（2.04–2.05.01）。
  - `src/global_game_event_info/mod.rs:100-105`：`group_key` 固定讀 `payload[1..3]`。
  - `src/faction_relation_group_info/mod.rs:156-181`：列表被截斷時默默回傳部分內容；`src/c_abi/faction_relation_group_info.rs:236-265` 只讀 `group_a` 那一列，假設矩陣對稱，但沒有驗證。
- **問題與根因：** 這些位置都是對某幾個版本「全列都成立」歸納出來的（文件寫明在五個版本、所有列上 pin 過）。但解析時沒有任何防呆，例如檢查 faction 的 `[u8; 3] == 0`、檢查 knowledge key 確實落在 knowledgeinfo 的 key 範圍、或檢查 FactionKey 是 0 或 faction 表的 key。下一個 patch 只要在前面插入或刪除一個位元組（2.06 就對 characterinfo 這樣做過，見 `:62-66`），這些函式就會回傳**看起來合理但錯誤**的值，而且仍然回 `OK`。factionnode 更特別：它用「header 是不是 2.06 形狀」來代表「版本是 2.06」，所以 2.07 如果維持 header 格式但移動了欄位，會靜默讀錯。
- **緩解因素：** 每個欄位都有 live 測試的 `KNOWN` pin（faction 4 列加上「128 列有 group」、factionnode 3 列加上「每個 node 都有地名」、character 6 列），在維護者機器上能抓到漂移。但依 D-03，這些測試在 CI 上不會執行，而且在沒有 install 時會 skip。
- **可重現的情境：** 合成一個 2.06 faction 列，在 key 之後多插一個 0 位元組：`group_key` 和 `relation_group_key` 會讀到錯位的值，`lookup_group_keys` 回 `OK`。
- **嚴重度：** low。
- **確信度：** plausible。
- **來源：** faction、factionnode、character 的部分是本次引入（#117、#118）；global game event 是既有的；relation group 的部分是 #121（本次引入）。
- **建議方向：** 加入便宜的結構檢查（`[u8; 3] == 0`、抽到的 key 屬於目標表、全表命中率門檻），不成立就整表標記為不支援的版本，而不是逐列回 0。

### D-07 — live 測試的列數 pin 太寬，名稱門檻會默默丟列

- **位置：**
  - 寬鬆的 pin：`src/store_info/mod.rs:185-189`（`>= 250`，2.06 實際 441）、`src/faction_info/mod.rs:175-179`（`>= 130`）、`src/quest_info/mod.rs:186-190`（`> 200`，實際 1,173）、`src/stage_info/mod.rs:222-226`（`> 10_000`，實際 53,313）、`src/mission_info/mod.rs:186-190`（`> 1_000`，實際 6,872）、`src/knowledge_info/mod.rs:172-176`（`> 1_000`，實際 6,839）、`src/character_info/mod.rs:267-271`（`> 5_000`，實際 7,315）、`src/sub_level_info/mod.rs:182-186`（`> 30`，實際 44）、`src/quest_gauge_info/mod.rs:161-165`（`> 200`，實際 511）。
  - 名稱長度門檻：PABGH 路徑一律 `1..=128`（例如 `src/item_group_info/mod.rs:48`、`src/faction_info/mod.rs:96`），mercenary 是 `1..=64`（`src/mercenary_info/mod.rs:95`），quest／stage 的 pabgh 路徑是 `1..=256`；超出範圍就 `continue`。
- **問題與根因：** 對照組：game advice（482）、item group（1,682）、region（1,014）、global game event（191）、trigger region（14）等表有精確的列數 pin，和 2.06 快照一致。但上面那些表的下限低於實際列數一個數量級，例如 stage 少掉 80% 的列仍然會通過。PABGH 路徑的表明明知道 header 列數，卻沒有比對「解析出的列數 = header 列數」；名稱太長或為空的列會被靜默略過，正好落在 B-01 指出的「版本漂移要明確失敗」原則之外。
- **可重現的情境：** 假設某個 patch 讓 storeinfo 有 190 列的名稱超過 128 bytes：解析剩 251 列，`store_info_lossy_live` 仍然通過。
- **嚴重度：** low。
- **確信度：** confirmed（測試程式碼與快照列數都是靜態事實）。
- **來源：** 既有問題。
- **建議方向：** 對 PABGH 路徑的表，測試改成 `entries.len() == header_count`；正式碼在數量不符時至少要有可查詢的狀態（例如 `*_parse_status`），而不是只能從 `entry_count` 間接推測。

### D-08 — `globalgameevent` 的 `paloc_key` 只接受剛好 14 位數

- **位置：** `src/global_game_event_info/mod.rs:128-133`（註解：「always fits in 14 digits because `event_key < 0x4300`」）、`:153-158`（`name_len != 14` 就回 `None`）。
- **問題：** 2.00 加入了 0x4308–0x430b 四列（同檔 `:185-188`），所以「`event_key < 0x4300`」這個前提已經不成立。這四列碰巧沒有 PalocStringRef，所以目前沒有出錯。但 14 位數只能涵蓋 hi32 在 0x0919..=0x5AF3（2,329..=23,283）之間的 key；超過 0x5AF3 的 key 會產生 15 位數，低於 0x0919 的會產生 13 位數，這兩種情況都會被當成「沒有 paloc」，以 `0` 回傳，不會報錯。資料裡 `name_len` 欄位本來就記錄了長度，沒有必要寫死成 14。
- **可重現的情境：** 合成一個 payload，`name_len = 15`、內容是 `"100000000000000"`：`extract_paloc_key` 回 `None`，bridge 回 `OK` 加上 `paloc_key = 0`。
- **嚴重度：** nit（目前的資料都落在範圍內）。
- **確信度：** plausible。
- **來源：** 既有問題。

### D-09 — `faction_info` 手寫版和巨集版的行為差異

- **位置：** `src/c_abi/faction_info.rs:214-237`（`get_entry` 只清 `*required`，沒有清 `*out_key`）、`:81-83`（`fn slice` 是 safe fn，內部呼叫 `from_raw_parts`）。
- **問題（對應審計重點 1）：** D 區塊有 10 個 bridge 用 `impl_name_only_bridge!`：craft tool、craft tool group、game advice、game advice group、gameplay variable、global game event group、item group、region、reserve slot、trigger region。手寫但形狀相同的有 house、royal supply、store、mercenary、faction spawn data、faction（另外多出 group 函式）。逐一比對 load、free、count、lookup、get_entry 的參數檢查、`catch_unwind`、first-wins dedup 和回傳碼，只發現 faction 有兩處差異：
  1. 其他 22 個 `get_entry` 實作（含巨集）都會在進入時把 `*out_key` 設成 0，faction 沒有。所以回 `OUT_OF_RANGE` 時，`*out_key` 會保留呼叫端原本的值。
  2. `slice(ptr, len)` 沒有標成 `unsafe fn`，任何 safe 呼叫端都能傳入懸空指標造成 UB。目前只在檢查過 `ptr`／`len` 的 extern 函式內部使用，所以實際上沒有問題，但違反 `slice_from_raw_or_empty` 文件（`mod.rs:189-193`）要求統一走 helper 的約定。
- **嚴重度：** nit。
- **確信度：** confirmed。
- **來源：** 本次引入（#116／#117）。

### D-10 — 策展表 drift 檢查的盲點

- **位置：** `src/c_abi/live_titles.rs:38-41`（`missions` 用 `.collect()` 建 `HashMap`，也就是 last-wins）；`src/c_abi/main_quest_chapter.rs:1358-1396`、`src/c_abi/side_quest_faction.rs:930` 起（只檢查「表內 key → live 標題」這一個方向）。
- **問題（對應審計重點 5）：**
  1. `curated_titles_match_live_install` 能偵測**已收錄 key 的改名**（`KNOWN` 的標題對不上時失敗），這部分符合要求。但它不能偵測：新 patch **新增**的主線或支線任務（表內沒有這個 key，就不會檢查）、任務被移到別的章節或陣營（策展欄位本來就無從驗證），以及英文以外的語言。
  2. `live_titles` 用 `parse_mission_info_lossy_with_header` 加上 last-wins 建 map，bridge 則是 first-wins。如果 mission 掃描器對某個 key 回傳了兩列（`mission_info` 模組 `:243-251` 記錄過 1.06 的 `MissionKey 65536` 重複），測試看到的名稱可能和 bridge 實際回傳的不同，測試就沒有驗證到 bridge 的真實行為。
  3. live 標題來源也繼承 D-01：如果某個策展 MissionKey 是 mission 掃描器排除的那 2,444 個之一，測試會回報 drift，不過這反而是正確的失敗。
- **嚴重度：** nit。
- **確信度：** 程式碼差異 confirmed；第 2 點的後果 plausible。
- **來源：** 既有問題。

### D-11 — 文件與程式碼脫節

全部是 nit、confirmed。依來源分組：

1. **`docs/c-abi-surface.md` 沒有和 `extern "C"` 符號一一對應（跨區塊關注點）：** D 區塊共有約 220 個 `extern "C"` 符號（18 個手寫 bridge 加 character 共 158 個，加上 10 個巨集 bridge × 6 = 60 個）。文件只用「Rust module structure」的一行摘要描述大多數 bridge（`:456-534`），沒有逐一列出函式名稱。例如 `crimson_questinfo_load_*_with_pabgh`、`crimson_stageinfo_load_*_with_pabgh`、`crimson_global_game_event_info_lookup_{group,paloc}_key`、`crimson_gimmickinfo_lookup_display_name`、`crimson_main_quest_*` 的十個函式，都要讀原始碼才知道。CLAUDE.md 說這份文件是「Full catalog」，實際上不是。（既有）
2. **列數過時（計畫書待查事項，已確認）：** 見文末「計畫書待查事項」一節的對照表，共 16 個檔案、3 類文件。（既有）
3. **巨集說明：** `src/c_abi/mod.rs:351-354` 和 `docs/c-abi-surface.md:537-543` 說 HouseKey、RoyalSupplyKey 用 `impl_name_only_bridge!`，但 `src/c_abi/house_info.rs`、`royal_supply_info.rs` 都是手寫的，只是行為和巨集相同。（既有）
4. **`docs/c-abi-surface.md:401-402`：** 說 lib.rs 只把「mission/quest/stage/knowledge/questgauge/sub_level/gimmick/character」八個模組放在 `c_abi` 之後，實際上 D 區塊全部 27 個表模組都在 `c_abi` 之後（`src/lib.rs:18-88`）。（既有）
5. **`src/faction_node_info/mod.rs:54-61`：** 寫「PALOC chain: none … **no `lookup_display_name` surface**」，但 #118 已經加了 `crimson_factionnode_lookup_display_name`（透過 knowledge）。同段把 `lo32 = 0x200` 說成 skill key 的碰撞 namespace，實際上 `0x200` 是 GimmickInfoKey 的顯示名稱 namespace（`src/c_abi/character_info.rs:4331`）。（混合：前者在 #118 變成過時）
6. **`src/c_abi/global_game_event_info.rs:248`：** 叫呼叫端用 `crimson_paloc_lookup_string_key`，這個符號不存在；實際的函式是 `crimson_paloc_lookup`（`src/c_abi/paloc.rs:213`），而且它吃的是十進位字串 key，不是 u64。（既有）
7. **`src/sub_level_info/mod.rs:68-71`：** 「The bridge's HashMap deduplicates by last-wins, but `or_insert` in the loader preserves first-wins」前後矛盾；bridge 實際是 first-wins（`src/c_abi/sub_level_info.rs:55`）。（既有）
8. **`src/c_abi/side_quest_faction.rs:151-155`：** 註解說保留 MD 的拼字「Encirlement」，等 live 比對時再修正，但該列已經是「Encirclement」，MD `:131` 也記錄了修正，所以註解過時。（既有）
9. **`src/mercenary_info/mod.rs:109`、`:115`：** 寫「Pins every 1.07 row」、「All 18 1.07 rows」，但 `KNOWN` 只有 5 列。（既有）
10. **`docs/ref-gamedata/main-quest-list.md:1`：** 寫「as of **Crimson Desert 2.04**」，`side-quest-list.md:117` 寫「as of 2.02」。實際以 live 測試為準，應該沿用到 2.06。`src/c_abi/main_quest_chapter.rs:28-32`、`:206-209` 只提到 2.03 的改名，沒有提 MD `:417-421` 記錄的 2.04 改名（1000185 "The Dark Veil" → "Veil of Darkness"）；程式碼本身的標題是正確的。（既有／2.04 期間）
11. **`src/c_abi/faction_relation_group_info.rs:195-196`：** 寫「only the low 16 bits are meaningful」，暗示會遮罩，但 lookup 是完整的 u32 比對，high bits 不為 0 時回 `NOT_FOUND`。行為正確（見排除 E-02），文件容易讓人誤會。（既有）
12. **`src/knowledge_info/mod.rs:189-195`、`src/quest_gauge_info/mod.rs:64-67`：** 說 HashMap 去重讓雜訊列「harmless」，在 first-wins 下不成立（見 D-01）。（既有）

---

## 審計重點逐條結論

| # | 審計重點 | 結論 |
|---|---|---|
| 1 | `impl_name_only_bridge!` 產生的函式；哪些表手寫；手寫版與巨集版行為是否一致 | 巨集本身已在 B 區塊審過（B-02、B-11）。巨集 bridge 共 10 個，手寫的 6 個（house、royal supply、store、mercenary、faction spawn、faction）行為一致，只有 faction 的 `get_entry` 沒有清 `*out_key`，以及 safe fn `slice`（D-09）。文件誤把 house／royal supply 列為巨集 bridge（D-11 第 3 點）。 |
| 2 | Key 寬度擴展（u16 → u32 高位必為 0）；`characterinfo` cat byte 剝除規則 | u8／u16 key 一律用 `u32::from` 擴展，lookup 做精確比對，沒有任何截斷（E-02）。cat byte 剝除規則和 2.06 新載入的雜湊 key 列互相衝突，三個查詢函式和 `lookup_faction_key` 不一致（D-02）。 |
| 3 | PALOC namespace（lo32）常數是否有測試釘住；巧合碰撞的 namespace 不能當預設值 | mission／quest／stage／knowledge／gimmick／character 的 `lookup_display_name` 都由呼叫端傳入 lo32，沒有內建預設值；有內建常數的是 character `0x30`（只在 `resolve_portrait` 使用）、faction group `0x214`、factionnode `0x490`，全部都有 live 測試釘住（E-08）。沒有任何 bridge 把已知的碰撞 namespace 當成預設值。但 gimmick 和 character 不檢查 key 是否屬於該表，會讓碰撞以 `OK` 的形式漏出（D-04）。這些 pin 都在 CI 上不執行（D-03）。 |
| 4 | DLC merge 的 key 衝突優先順序、沒有 DLC 時的行為 | 衝突時保留 base 列（first-wins），文件說 2.06 上兩邊在 bridge 暴露的欄位上完全一致（dev-log #119 記錄有量測）。沒有 DLC 時不呼叫 merge 即可，所有函式都只用 base 列回答，各 bridge 有 null／empty 參數測試（`c_abi_factionnode_dlc_merge_args`）。問題在錯誤路徑：merge 一律回 `OK`（B-02），空 header 會把雜訊列永久加入（D-05），來源編號會被失敗的 merge 用掉（B-10）。只有 4 張表能 merge，mission／quest／stage／gimmick 沒有 merge（文件 `c-abi-surface.md:256` 有寫明；`dlc_01` 有沒有這些表的 DLC 專屬 key，見「未能驗證」）。 |
| 5 | 策展表：`curated_titles_match_live_install` 能偵測改名；策展來源文件與程式碼常數是否同步 | 能偵測已收錄 key 的改名，不能偵測新增任務或非英文字串（D-10）。MD 和程式碼同步：side quest 84 列對 84 列、改名紀錄一致；抽查的 8 個主線 key 都在 2.06 快照中（E-06）。只有 MD 標頭的版本號和部分程式碼註解過時（D-11 第 8、10 點）。 |

## 跨區塊關注點逐條結論

| 主題 | 結論 |
|---|---|
| 不可信輸入的配置上限 | 沒有問題（E-03）。所有 `Vec::with_capacity(count)` 都在「header 長度 == 固定公式(count)」的相等檢查之後才執行，所以配置量和輸入大小成正比；`HashMap::with_capacity(raw.len())` 以實際解析出的列數為準；relation group 的列表長度上限是 64（`faction_relation_group_info/mod.rs:173`）。只有 32 位元目標上 `4 + 8 * count` 這類算式可能 wrap（同 B-12 第 5 點），專案只建置 x64。 |
| Panic 面與 abort | 沒有發現面對不可信輸入會 panic 的位置（E-04）。各掃描器的 `read_row_head(...).unwrap()` 有掃描器先前的邊界檢查保證；所有 extern 入口（`*_free` 除外，Drop 不會 panic）都包了 `catch_unwind`。 |
| Roundtrip 對稱性 | 不適用（E-05）。D 區塊全部是唯讀的 lossy 解析，沒有寫回路徑。 |
| 版本漂移明確失敗 | **不符合。** 除了 B-01、B-02 的共用根因之外，D 區塊還有三種靜默失敗：啟發式掃描在 2.06 上靜默漏列（D-01）、固定偏移量欄位靜默讀錯（D-06）、名稱門檻靜默丟列，而且沒有列數比對（D-07）。 |
| Feature gate 組合 | 模組只在 `c_abi` 下編譯，`default`／`python` 不會編到，不影響 build；`live_titles` 是 `#[cfg(test)]`。full surface 的 clippy 能抓到 dead code。但 CI 的 `cargo test` 沒有開 `c_abi`，所以 D 區塊的測試在 CI 上從未執行（D-03）。 |
| `docs/c-abi-surface.md` 一致性 | **不一致。** 沒有逐一列出符號、列數過時、巨集清單錯誤、lib.rs gate 說明錯誤（D-11 第 1–4 點）。`_with_header` 九個 loader 的清單和 DLC merge 的四個 bridge 和程式碼一致。 |

---

## 計畫書「掃描時注意到的待查事項」

> **(D)** 部分 bridge 的模組文件還寫著 1.07 的列數（例如 `craft_tool_info` 的「17 rows in 1.07」），可能已經過時。

**結論：確認屬實**（confirmed，既有問題，歸入 D-11 第 2 點）。這條待查事項成立，所以不列入排除說明。以 `data/gamedata-keys-2.06/*.txt` 的列數為準：

| 表 | 文件寫的列數 | 出現位置 | 2.06 實際 |
|---|---:|---|---:|
| crafttoolinfo | 17 | `src/craft_tool_info/mod.rs:5`、`src/c_abi/craft_tool_info.rs:6`、`docs/c-abi-surface.md:508` | 19 |
| crafttoolgroupinfo | 10 | `src/craft_tool_group_info/mod.rs:5`、`src/c_abi/craft_tool_group_info.rs:6`、`c-abi-surface.md:509` | 12 |
| gameadviceinfo | 461 | `src/game_advice_info/mod.rs:5`、`src/game_advice_group_info/mod.rs:6`（「break the 461 tips」）、`src/c_abi/game_advice_info.rs:5`、`c-abi-surface.md:518` | 482 |
| gameplayvariableinfo | 47 | `src/gameplay_variable_info/mod.rs:5`、`src/c_abi/gameplay_variable_info.rs:4`、`c-abi-surface.md:511` | 68 |
| globalgameevent | 103（含「~24 of 103」、「1 of 7 distinct」） | `src/global_game_event_info/mod.rs:5`、`src/c_abi/global_game_event_info.rs:11,18,20,210-212`、`c-abi-surface.md:512` | 191（12 個 group） |
| globalgameeventgroup | 7 | `src/global_game_event_group_info/mod.rs:5`、`src/c_abi/global_game_event_group_info.rs:4`、`c-abi-surface.md:517` | 12 |
| itemgroupinfo | 1,500 | `src/item_group_info/mod.rs:6`、`src/c_abi/item_group_info.rs:5`、`c-abi-surface.md:522` | 1,682 |
| mercenaryinfo | 18 | `src/mercenary_info/mod.rs:10,18`、`src/c_abi/mercenary_info.rs:12`、`c-abi-surface.md:491` | 21 |
| reserveslot | 27 | `src/reserve_slot_info/mod.rs:4`、`src/c_abi/reserve_slot_info.rs:5`、`c-abi-surface.md:520` | 28 |
| regioninfo | 1,004 | `src/region_info/mod.rs:5`、`c-abi-surface.md:521` | 1,014 |
| triggerregioninfo | 12 | `src/trigger_region_info/mod.rs:4`、`src/c_abi/trigger_region_info.rs:5`、`c-abi-surface.md:510` | 14 |
| storeinfo | 292（含「1,754 bytes (matches the live file)」） | `src/store_info/mod.rs:9,18-19,131`、`src/c_abi/store_info.rs:12`、`c-abi-surface.md:490` | 441 |
| factionnode | 1,158 | `src/faction_node_info/mod.rs:8` | 1,120 |
| factionspawndatainfo | 117 | `src/faction_spawn_data_info/mod.rs:7-8` | 131 |
| sublevelinfo | ~40 | `src/sub_level_info/mod.rs:13` | 44 |
| factionrelationgroup | 「always 5 in 1.07」 | `src/c_abi/faction_relation_group_info.rs:173` | 5（數字正確，只有版本標註過時） |

仍然正確的：gameadvicegroupinfo 8、houseinfo 4、royalsupply 4、faction 135（2.06）。各表的 live 測試 pin 大多已經更新到新列數（例如 crafttool 19、gameplayvariable 68、itemgroup 1,682），所以過時的只是說明文字，不是斷言。染色三表屬於 E 區塊，這裡不列。

## 排除說明（查過、確認沒有問題的項目）

- **E-01 去重語意一致：** D 區塊每個 bridge（10 個巨集、17 個手寫、character、4 個 DLC `absorb`）都用 `Entry::Vacant` 做 first-wins，`by_key` 和 `entries` 永遠同步，`get_entry` 不會列出重複 key。mission bridge 的註解（`src/c_abi/mission_info.rs:67-74`）記錄了過去 last-wins 造成列舉重複的修正，現在已經沒有任何 bridge 用 last-wins。（first-wins 在雜訊列先出現時的問題歸入 D-01。）
- **E-02 Key 寬度擴展沒有截斷：** mercenary（u8）以及 house、store、royal supply、item group、region、craft tool、craft tool group、global game event、group、faction relation group（u16）都用 `u32::from(key)` 擴展，並且要求 body 裡的 key 等於 PABGH key（例如 `src/item_group_info/mod.rs:48`）。所有 lookup 都對完整 u32 做 `HashMap::get`。在 D 區塊的 `src/c_abi/*.rs` 搜尋 `as u16`、`as u8`、`& 0xFFFF`，結果是 0 處，所以 high bits 不為 0 的輸入會回 `NOT_FOUND`，不會被截斷後誤中。唯一刻意遮罩的是 character 的 `& 0x00FF_FFFF`（其問題見 D-02）。
- **E-03 配置上限：** 見跨區塊表。另外，quest／stage 的 `4 + 8 * count`（count 來自 u32）在 64 位元上不會溢位；`parse_pabgh` 先要求 `len == 2 + count*8`（u16 count）才配置。
- **E-04 Panic 面：** 掃描器迴圈條件 `o + 12 < n` 保證 `data[o..o+4]` 可讀；`data[o + 8 + slen]` 前面已經檢查 `o + 9 + slen <= n`；`bytes[0]` 在 `slen >= 2` 時存在；`read_row_head(&data[start..], 4)` 需要 `4 + slen + 4 <= n - o`，前面的檢查已經保證，所以 `unwrap()` 是安全的。`opens_with_path` 用 `get`。global game event 的 `&body[head.body_at..]` 由 `read_row_head` 保證範圍。faction relation group 的列表迴圈先檢查 `cursor + n*2 > body.len()`，`n` 上限 64。
- **E-05 Roundtrip 不適用：** D 區塊的 `*_info` 模組只有 `parse_*`，沒有任何 `write_*` 或 `to_bytes`，不受「Binary format must roundtrip」規則約束。
- **E-06 策展表的 key 與 MD 同步：** `entry_keys_are_unique` 保證同類 key 唯一；`unresolved_rows_are_the_known_five` 和 MD `:405-412` 的五列一致；side quest MD 的 `* quest:` 共 84 列，和程式碼 `ROWS` 的 84 列一致；兩份 MD 的「Reconciliation」和「Retitles」表與程式碼註解逐列對得上（包括 `Hidden Ace` 的 2.03 改名，MD `:440`）。抽查的 8 個 key（questinfo 的 1000027、1000509、10011000、11006000、10029000、10040400、10060100，以及 missioninfo 的 1001231）都在 2.06 快照中。mission 和 quest 的 key 空間會重疊（例如 1000027 兩邊都有），程式碼用 `Entry::Mission`／`Entry::Quest` 分開索引，並且有 `call_by_key(ch, 10_001) == NOT_FOUND` 的測試。
- **E-07 Knowledge／quest gauge／sub-level 的 key 規則在 2.06 不漏列：** 用快照計數，knowledgeinfo 沒有 key ≥ 2³¹（`0x7FFF_FDxx` 類的 562 個 key 都低於 2³¹，被 `< 0x80` 涵蓋），questinfo、questgaugeinfo、sublevelinfo 都沒有 key ≥ 2²⁴。這三張表如果在 2.06 漏列，原因只會是名稱規則，而 knowledge 的 6,839 列已經在 #118 和 header 對過。
- **E-08 PALOC namespace 預設值：** 見審計重點 3。`CHARACTER_DISPLAY_NAME_LO32 = 0x30` 只用在 `resolve_portrait`；`FACTION_GROUP_PALOC_LO = 0x214` 刻意避開 faction 文件記錄的 `0x30` 碰撞；factionnode 的 `0x490` 寫死在 `src/c_abi/faction_node_info.rs:383`，有「每個 node 都有地名」的全表 live 斷言（`:599-611`）。
- **E-09 `_with_header` 和 DLC merge 的清單與文件一致：** D 區塊的 8 個 `*_load_from_bytes_with_header`（mission、character、knowledge、stage、gimmick、quest、quest gauge、sub-level）和 `c-abi-surface.md:53-55` 一致；4 個 `*_merge_dlc_from_bytes`／`*_entry_source`（factionnode、knowledge、faction、character）和 `:210-225` 一致。
- **E-10 雙呼叫字串協定：** D 區塊各檔案自己的 `write_str_to_buf` 副本（quest、mission、knowledge、stage、faction_node、faction_relation_group、character、main／side quest）和 `mod.rs:217-233` 的共用版本逐字元相同（`required = len + 1`，不足時回 `BUFFER_TOO_SMALL` 且不寫入）。main／side quest 的多欄位 `get_entry` 在部分 buffer 不足時，會先把足夠的欄位寫入，再回 `BUFFER_TOO_SMALL`，文件 `:543-547` 有說明。

## 未能驗證（需要在有 live install 的機器上執行，或需要核准執行腳本）

1. **D-01 的實際列數：** 對 7 個 `_with_header` loader 各餵入 2.06 的 `(body, header)`，把 `get_entry` 列出的 key 集合和 `data/gamedata-keys-2.06/*.txt` 做差集。預期 mission 少 2,444 個、stage 至少少 1,177 個、quest 至少少 1 個、gimmick 至少少 12 個（2.04 文件推算約 140 個）、knowledge 一致。同時列出 `entries` 中不在快照裡的 key，也就是雜訊列。
2. **D-02 的碰撞數：** 對 characterinfo 快照中 359 個 hi8 不為 0 的 key，計算有幾個的 lo24 也是表內的 key（本次嘗試用 `python -I` 對快照做集合運算，Bash 執行未獲核准）。再用 PALOC 確認雜湊 key 列的顯示名稱實際的 hi32 是完整 key 還是其他值。
3. **存檔端影響：** 在 `crimson-rs-private` 的 save fixtures 和 2.06 存檔中，計算 `_missionStateList`、`_stageStateData`、`FieldGimmickSaveData._gimmickInfoKey`、`_characterKey` 有多少值落在 D-01、D-02 的漏列範圍內。
4. **DLC 覆蓋範圍：** `dlc/dlc_01/0008` 有沒有 missioninfo、questinfo、stageinfo、gimmickinfo 的 `.dlc_01` 副本，以及裡面有沒有 base 沒有的 key。如果有，這些 bridge 沒有 merge API，玩過 DLC 的存檔就會查不到。
5. **D-05：** 對 knowledge 和 character 各用空 header 呼叫一次 merge，記錄 `added` 和新增列的名稱。
6. **計畫書的驗證項：** `cargo test --lib --features c_abi -- --test-threads=4`，以及 `docs/save-editor-keys-reference.md` 的 1.06 回歸基準。依限制條款，本次沒有執行任何測試。
