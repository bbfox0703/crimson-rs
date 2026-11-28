# Code audit — E. 染色／Prefab 子系統（2026-11-28）

依 [`code-audit-2026-11-28-plan.md`](code-audit-2026-11-28-plan.md) 的 E 區塊做靜態審計。**沒有修改任何程式碼**，沒有執行 crimson-rs 的任何程式或測試，沒有讀 live install 的遊戲資料，也沒有碰 PAPGT trailer。

- **基準：** `dev` @ `e465942`。下文行號都以這個 commit 為準。
- **範圍：** `src/dye_color_group_info/mod.rs`、`src/part_prefab_dye_slot_info/mod.rs`、`src/part_prefab_dye_texture_pallete_info/mod.rs`、`src/c_abi/{dye_color_group_info,part_prefab_dye_slot_info,part_prefab_dye_texture_pallete_info,item_part_prefab}.rs`。為了判斷行為，也讀了它們直接依賴的 `src/binary/row_head.rs`、`src/binary/static_header.rs`（`legacy_pair`）、`src/skill_info/pabgh.rs`（`parse_pabgh` / `entry_ranges`）、`src/binary/paz.rs:697-737`（`extract_file`）、`src/lib.rs:1-93`（feature gate）、`src/c_abi/mod.rs:35-60,199-205`、`.github/workflows/ci.yml`，以及文件 `docs/c-abi-surface.md`、`docs/dye-editor-scope.md`。
- **方法：** 逐行閱讀原始碼；用 `git blame` / `git log` 判定來源；把 59 個 `extern "C"` 符號逐一對照兩份文件；交叉參考已完成的 A、B、C、F、G 報告，避免重複記同一個根因。
- **沒做到的：** 計畫書的驗證方式（`docs/dye-editor-scope.md` 範例、live-install 測試）需要執行程式，本回合不做。下面凡是「實際資料會不會觸發」的後果，都標 plausible。

### 確信度與來源歸屬的定義

沿用 A、B 區塊報告的定義：

- **confirmed**：只靠原始碼或文件文字就能確定的事實，例如「沒有這個檢查」、「測試會走這個分支」、「文件和程式碼不一致」。
- **plausible**：後果是追程式路徑推出來的，**沒有用位元組或實際重現驗證過**。依 CLAUDE.md 與計畫書，這類一律標 plausible，即使程式路徑本身是確定的。
- **本次引入**：引入的 commit 落在 2.06 移植週期內，也就是 `95fa25f`（2026-11-27）到基準 `e465942` 之間。E 區塊在這段期間的 commit：`95fa25f`（`legacy_pair` 接線）、`5a77bbd`（dye 22-byte 紀錄的逐筆走訪）、`ef31f24`（12-byte mask 改用 dye slot 編號解讀）、`87b042e`（fixture 測試）、`f33f21b`（透過 `.prefab` 的 item → mesh join）、`22db5f3`（DLC merge）、`3ca07dd`（色票解鎖 KnowledgeKey 與主題名稱）。
- **既有問題**：引入時間早於 `95fa25f`。主要的來源 commit：`dd3ea2d`（2026-05-16，三個 dye bridge）、`a20b760`（2026-05-16，name join）、`d8c15cd`（2026-05-17，palette accessor）、`749a5c4`（2026-05-18，`resolve_dye_slot_count`）、`ebbb4f5`（1.13 extra layer）、`188a311`（2.01 `_mask_full`）。

---

## 摘要

| # | 標題 | 嚴重度 | 確信度 | 來源 |
|---|---|---|---|---|
| E-01 | `resolve_dye_slot_count` 回傳的是第一個 mesh 的 sub-mesh 數，不是 dye slot 數；rustdoc、v3 範例與 `c-abi-surface.md` 仍說它是「這個物品有幾個 dye slot」 | medium | confirmed（文件與程式碼矛盾）／plausible（下游後果） | 既有問題，2.01 起語意失效，本次的 prefab join 改變了 `[0]` 的意義 |
| E-02 | Dye 色票的「等長切分」fallback 會在長度混雜但總長剛好整除時靜默讀錯，違反模組文件「讀不懂就給空色票」的承諾 | low | plausible | 本次引入 |
| E-03 | `palette_knowledge_at` 的 live 測試把 `UNSUPPORTED_VERSION` 當成「舊版 install」直接 skip，正好漏掉它要抓的漂移 | low | confirmed | 本次引入 |
| E-04 | 四個 E loader 遇到版本漂移都回 `OK`，只給空的或部分的 handle；C ABI 沒有任何「被丟棄的列數」訊號 | low | confirmed（程式碼）／plausible（後果） | 既有問題；prefab join 是本次引入 |
| E-05 | Prefab join 把抽取錯誤當成「沒有這個 prefab」吞掉，並快取結果；DLC merge 會退回同名的 base prefab；merge 時用的是載入當下解析的 PAMT | low | plausible | 本次引入 |
| E-06 | `.prefab` 字串掃描與路徑正規化：`.pac` 副檔名區分大小寫、排除空白與非 ASCII、不處理 `\`、同名 prefab 取第一個且不報告衝突 | low | plausible | 本次引入 |
| E-07 | 新的 `_with_prefab_pamt` 與 `merge_dlc` 入口會解析呼叫端給的 PAMT，繼承 A-01／A-02 的行程 abort | low | plausible | 本次引入的入口，根因既有 |
| E-08 | E 區塊的測試（包括不需要 install 的合成測試）在 CI 上一個都不跑 | low | confirmed | 既有問題，本次新增的合成測試也在內 |
| E-09 | `crimson_item_part_prefab_lookup_dye_slot_material` 的 `dye_slot_no` 是 `u8`，同名的 slot-info 版本是 `u32` | nit | confirmed（程式碼）／plausible（後果） | 本次引入 |
| E-10 | 重複 key 的處理不一致：三個表 handle 都取第一筆，`item_part_prefab` 的 `rows` 取最後一筆 | nit | plausible | 本次引入 |
| E-11 | 對 name-join handle 呼叫 `merge_dlc` 時，文件說會忽略 `dlc_prefab_pamt_path`，實際上仍會開檔，失敗就回錯 | nit | confirmed | 本次引入 |
| E-12 | 回傳碼語意重疊：`NOT_FOUND` 同時代表「沒這個 key」和「這格不需要解鎖」等；非 UTF-8 的 tribe 字串回 `INVALID_PATH`；`position_for_rgb` 遇到重複 RGB 時只回第一個 | nit | confirmed（程式碼）／plausible（實際出現） | 既有問題與本次引入都有 |
| E-13 | 已知是常數的位元組沒有驗證（slot marker `0xFF`、row pad、palette pad），降低了偵測漂移的能力 | nit | confirmed | 既有問題 |
| E-14 | 文件與程式碼脫節：列數、2.06 之前的欄位說明、`key=0` 的 sub 數、錯誤碼清單、`resolved_item_count` 沒有任何文件記載 | nit | confirmed | 多數既有，少數本次引入 |
| E-15 | FFI 衛生：opaque handle 標 `#[repr(C)]`、私有 safe fn 直接解參考 raw pointer、部分 getter 沒有先清空 out-param、重複的 helper | nit | confirmed | 既有問題與本次引入都有 |
| E-16 | 測試小問題：測試裡的 `1 << n` 可能在 assert 前溢位；key 不變式的測試不必要地依賴 0009 PAMT | nit | confirmed | 本次引入 |

沒有 critical 或 high 的 finding。E 區塊的解析器都只讀不寫，所有配置都有上限，索引也都有先檢查，所以正常與壞檔輸入都看不到 E 區塊自己的 panic 路徑（見文末排除說明）。比較值得處理的是 **E-01**：這是下游 C# 編輯器可能照著文件直接使用的 API，而它回答的不是文件說的那個問題。其次是 E-02 到 E-05 這一組：它們都會讓下個 patch 的漂移靜默發生，而不是明確失敗。

---

## Findings

### E-01 — `resolve_dye_slot_count` 回答的不是「有幾個 dye slot」

- **位置：** `src/c_abi/item_part_prefab.rs:1131-1161`（rustdoc：「One-shot "how many dye slots does this item have?"」、「`DIRECT` (0): `out_slot_count` is authoritative」）、`:1185-1209`（實作：取 `part_prefab_keys_by_item[item][0]`，再呼叫 `crimson_part_prefab_dye_slot_info_lookup_slot_count`）、`:1106-1110`（`DIRECT` 常數說明）；`src/c_abi/part_prefab_dye_slot_info.rs:188-189`（`lookup_slot_count`：「**Direct replacement for `dye_slot_counts.json[item_id]`.**」）；`docs/dye-editor-scope.md:609`、`:656-708`（v3 一節與 C# 範例：「slotCount is the answer — render slot picker with that many entries」）；`docs/c-abi-surface.md:502-505`。
- **問題與根因：**
  1. `slot_count()` 是 `slots.len()`，也就是 mesh 的 **sub-mesh 數**（`src/part_prefab_dye_slot_info/mod.rs:176-178`）。2.01 起，一個物品可以設定的 dye slot 是 12-byte mask 的位元聯集（`dye_slot_bits`），和 sub-mesh 數無關。核心模組自己的文件就寫著「a one-slot prefab can expose three dye slots」（`mod.rs:180-191`）。
  2. 本週期的 `docs/dye-editor-scope.md:342-344` 已經寫明「`..._lookup_slot_count` counts sub-meshes, not dye slots, and so does `crimson_item_part_prefab_resolve_dye_slot_count`」。但同一份文件的 v3 一節、`c-abi-surface.md`，以及函式自己的 rustdoc 都沒有同步，仍然說回傳值是 authoritative 的 dye slot 數。
  3. 換成 prefab join（`f33f21b`）之後，`part_prefab_keys_by_item[item]` 是「所有 variant 的所有 mesh 去重後」的列表，`[0]` 只是第一個 variant 的第一個 `.pac`（通常是上半身）。所以在 prefab-join handle 上，回傳值連「這個物品的 sub-mesh 數」都不是。`dye-editor-scope.md:499-500` 卻說它「work on either handle」。
  4. `resolve_dye_slot_count` 從另一個 handle 查 `slot_count`，但 item handle 自己已經持有同一張表的 `rows`（`:103`）。如果兩個 handle 是用不同版本的表建出來的，回傳值會不一致，而且沒有任何檢查。
- **可重現的情境（取自程式碼裡已釘住的值，本回合沒有執行）：**
  - Name-join handle：`item_part_prefab.rs:1837-1840` 釘住 item 121013 → keys `[0x8dfa1dd5]`、`rows[0x8dfa1dd5].slot_count() == 1`、`dye_slot_bits() == 0b111`。所以 `resolve_dye_slot_count(121013)` 會回 `(1, DIRECT)`，但這個物品有 3 個 dye slot（存檔裡也確實有 `_dyeSlotNo` 0、1、2 三個元素）。
  - Prefab-join handle：item 111147 的 keys 是 `[0xf407029b, 0x4170373c, 0x86db33f9, 0xf736011d]`，dye slot 聯集是 `0xff`，也就是 8 個（`:2083-2087`）。`resolve_dye_slot_count` 只會回 `0xf407029b` 這一個 mesh 的 sub-mesh 數。
  - 照 v3 的 C# 範例「render slot picker with that many entries」實作的編輯器，在這兩個物品上都會列出錯誤數量的 dye 欄位。
- **嚴重度：** medium（這是公開的 C ABI，文件直接教下游這樣用；後果是編輯器建立錯誤數量的 `ItemDyeSaveData` 元素）。
- **確信度：** confirmed（文件與程式碼的矛盾，以及 `[0]` 的取法）／plausible（下游實際受影響的程度；CrimsonAtomtic 不在本次審計範圍）。
- **來源：** 既有問題（`749a5c4`，2026-05-18；2.01 的 mask 改變語意後就已失準）。本週期的 `f33f21b` 改變了 `[0]` 的意義，`ef31f24` 在 `dye-editor-scope.md:342-344` 記下了「這不是 dye slot 數」，但沒有同步 rustdoc 與 v3 範例。

### E-02 — Dye 色票的等長切分 fallback 會靜默讀錯

- **位置：** `src/dye_color_group_info/mod.rs:340-358`（`record_starts`；等長切分在 `:354-357`），對照模組文件 `:46-52`（「A row that fits neither gets an empty palette rather than a misread one」）。條件判讀在 `:312-321`。
- **問題與根因：** 逐筆走訪（`walk`）只認得 `00` 和 `01` + 17 bytes 的 `CheckKnowledge` 兩種條件。只要有一筆是別種條件，就改用「`span % count == 0 && span / count >= 4`」等長切分。這個條件只檢查總長能不能被筆數整除，**不檢查每筆的長度真的相同**。長度混雜、但總長剛好整除時，第二筆之後的起點會落在前一筆的條件 bytecode 中間，BGRA 讀到的是條件的位元組。`stride != 8` 時條件一律標 `Unknown`，但 **palette 本身照常回傳**，`palette_at` 回 `OK`。
- **可重現的輸入（合成，未執行）：** 用 `record_tests::row` 的形狀造 `count = 2` 的列：第 1 筆 `[3,2,1,255, 00]`（5 bytes），第 2 筆 `[6,5,4,255, 02, <18 個任意位元組>]`（23 bytes）。`walk` 在第 2 筆失敗（首位元組 `02`），總長 28 可被 2 整除、每筆 14 ≥ 4，所以切在 `from + 14`。`palette[1]` 會是第 2 筆條件區裡第 10–13 個位元組，不是 `[4,5,6,255]`。現有的 `refuses_a_condition_it_cannot_walk`（`:440-447`）刻意多推一個 `0` 讓總長不能整除，所以沒有涵蓋這個情況。
- **實際觸發機率：** 2.06 的 1,090 筆全部能逐筆走訪，不會走到 fallback。下個 patch 如果加入第三種條件形式，總長 ≡ 0 (mod 109) 的機率大約是 1/109；但如果每筆都換成同一種新長度，切分反而是對的。
- **嚴重度：** low　**確信度：** plausible（合成輸入沒有實際跑過）　**來源：** 本次引入（`5a77bbd`）。

### E-03 — Knowledge 測試在它要抓的漂移上 skip

- **位置：** `src/c_abi/dye_color_group_info.rs:849-853`；相關的 `src/dye_color_group_info/mod.rs:312-321`、`src/c_abi/dye_color_group_info.rs:429-431`。
- **問題與根因：** `c_abi_dye_color_group_info_palette_knowledge_live` 在 `knowledge(HER, 0) == Err(UNSUPPORTED_VERSION)` 時印出「pre-2.06 palette」然後 `return`。但 `UNSUPPORTED_VERSION` 同時涵蓋 `ConditionInfoKey`（真正的 2.05 以前資料）和 `Unknown`。只要下個 patch 改了條件 bytecode、但每筆長度仍然相同（例如尾端的 9 個位元組變了，仍是 22 bytes），逐筆走訪會失敗，等長切分以 stride 22 成功，條件全部是 `Unknown`，這個測試就會 **skip 而不是 fail**。這個解析器只支援最新版，所以「pre-2.06」這個 skip 分支在支援範圍內只會在漂移時觸發。
  - 其他測試也補不上這個洞：`dye_color_group_info_lossy_live`（`mod.rs:623-654`）只檢查名稱和 `entries.len() >= 5`；`palette_pins_slot103_observations`（`c_abi/...:725-823`）只檢查 HER、POR 兩列的 RGB 和 `size >= 50`。在上述漂移下 RGB 仍然正確，所以都會通過。
- **可重現的情境：** 把 live 表每筆條件尾端的 `TAIL`（`mod.rs:365`）改掉一個位元組，長度不變。預期：knowledge 測試印出 skip 訊息並通過，其他 dye 測試也全部通過。
- **嚴重度：** low（執行期 API 會回 `UNSUPPORTED_VERSION`，不會給錯的 key，只是 CI 和本機都不會報警）　**確信度：** confirmed（測試的分支邏輯）　**來源：** 本次引入（`3ca07dd`）。

### E-04 — 四個 loader 在版本漂移時都回 `OK`

- **位置：**
  - `src/dye_color_group_info/mod.rs:186-221`：header 解析失敗就回空 `Vec`；head 不符就略過該列；色票解析失敗會 `unwrap_or_default()`（`:211`），列保留、色票是空的。
  - `src/part_prefab_dye_slot_info/mod.rs:427-440`、`src/part_prefab_dye_texture_pallete_info/mod.rs:231-244`：一樣，回空的或略過的列。
  - C ABI loader：`c_abi/dye_color_group_info.rs:103-105,141-143`、`c_abi/part_prefab_dye_slot_info.rs:101-103,139-145`、`c_abi/part_prefab_dye_texture_pallete_info.rs:91-93,129-134`，以及 `c_abi/item_part_prefab.rs:282-291`：拿到什麼都回 `error::OK`。
- **問題與根因：** 這些解析器的契約本來就是「lossy safety net」（`part_prefab_dye_slot_info/mod.rs:54-56`），但 C ABI 沒有把「丟掉了幾列」或「表頭不認得」傳給呼叫端。C# 端看到的 `entry_count == 0`、`palette_size == 0`，或某個 key 回 `NOT_FOUND`，和「這個 key 本來就不存在」無法區分。prefab join 也一樣：如果下個 patch 改了 row key 的推導方式（例如不再是小寫路徑的 checksum），`pac_keys`（`item_part_prefab.rs:210-216`）會把所有路徑都過濾掉，handle 仍然回 `OK`，只是 `resolved_item_count` 掉到接近 0。計畫書的跨區塊要求是「要能明確失敗，不能悄悄地讀錯」；這裡沒有讀錯，但也不是明確失敗。
- **可重現的輸入：** 把 2.06 的 `partprefabdyeslotinfo` header 截掉最後 1 byte 傳給 `crimson_part_prefab_dye_slot_info_load_from_bytes`：`detect_named_header` 失敗，`legacy_pair` 原樣傳回，`parse_pabgh` 因長度不符回 `Err`，最後得到一個空 handle 和 `OK`。
- **嚴重度：** low（有 live 測試把關：slot-info 測試要求「每一列都要解析成功」，`:528-533`；pallete 測試要求 11 列；dye 測試釘住全部 10 個 key。但 C# 端在執行期沒辦法自我檢查）。
- **確信度：** confirmed（程式路徑）／plausible（下游的實際影響）。
- **來源：** 既有問題（`dd3ea2d`、`a20b760`）；prefab join 的部分是本次引入（`f33f21b`）。

### E-05 — Prefab join 吞掉抽取錯誤，merge 用的是過期的 PAMT

- **位置：** `src/c_abi/item_part_prefab.rs:174-185`（`PrefabArchive::pac_paths`：`extract_file(...).ok()?`）、`:197-206`（`Resolver::keys`：`find_map`）、`:248`（`join_items` 依名稱快取結果）、`:366-375`（`merge_dlc`：DLC 找不到就 `resolver.keys`）、`:146-151`（`PrefabArchive` 只存 `PackMeta` 和 `group_dir`）。
- **問題與根因：**
  1. `.ok()?` 把 I/O 錯誤（`.paz` 缺檔、權限、Steam 正在更新）、解密與解壓縮失敗，都轉成 `None`，也就是「這個 archive 沒有這個 prefab」。結果是：該物品靜默地沒有解析到任何列；`join_items` 會把空結果快取起來，同名的其他物品也一起失效；loader 仍然回 `OK`。
  2. 在 `merge_dlc` 裡，DLC archive 抽取失敗會落到 `resolver.keys`，也就是去 base archive 找**同名**的 prefab。如果 base 有同名但內容不同的 prefab，DLC 物品會接到 base 的 mesh。
  3. `PrefabArchive` 不保留檔案 handle，每次都依當初解析的 `PackMeta` 重新開 `.paz`（`paz.rs:705-717`）。如果 base handle 載入之後遊戲更新了（PAMT 的 offset 變了），之後的 `merge_dlc` 會用舊 offset 讀新 `.paz`。在 ChaCha20 / LZ4 那層多半會失敗，再被第 1 點吞掉，表現為 DLC 物品靜默地解析不到。
  4. A 報告第 126 行指出相反的情況：`extract_file` 內部的 **panic** 不會被 `.ok()?` 攔下，會讓整個 handle 載入失敗並回 `PANIC`。所以錯誤的粒度是「I/O 錯誤影響一個物品、panic 影響整個載入」，兩者都沒有診斷資訊。
- **可重現的情境：** 把 `<install>/0009/` 底下某個存放 prefab 的 `N.paz` 暫時改名，再呼叫 `crimson_item_part_prefab_load_from_file_with_prefab_pamt`。預期：回 `OK`，`resolved_item_count` 下降，沒有任何錯誤碼或計數告訴呼叫端有 prefab 讀不到。
- **嚴重度：** low　**確信度：** plausible　**來源：** 本次引入（`f33f21b`、`22db5f3`）。

### E-06 — `.prefab` 字串掃描與路徑正規化

- **位置：** `src/c_abi/item_part_prefab.rs:118-120`（`part_prefab_key_for_pac`）、`:126-143`（`prefab_pac_paths`）、`:160-168`（`by_stem`）、`:175`（查詢時小寫）。
- **問題與根因：** `.prefab` 是從 PAZ 讀出的不可信二進位。掃描器逐位元組找 `[u32 len][len bytes]`，條件是 `len` 在 5..=1024、內容全是 `is_ascii_graphic`、而且以 `.pac` 結尾。健壯性沒有問題（不會 panic，見排除說明），但比對規則有幾個缺口：
  1. `ends_with(b".pac")` 區分大小寫，但算 key 時會先 `to_ascii_lowercase`。`Foo.PAC` 這種寫法會直接被跳過。
  2. `is_ascii_graphic` 不含空白（0x20），也不含任何非 ASCII 位元組。路徑裡有空白或 UTF-8 字元的 mesh 會被跳過。
  3. 沒有把 `\` 正規化成 `/`，也沒有處理 `./`、重複斜線。只要寫法和 `tail_name` 不同，checksum 就對不上。
  4. 比對到一個字串後會整段跳過（`i += 4 + len`）。理論上一個假陽性的長度欄位可能吞掉後面真正的路徑；但假陽性本身也要以 `.pac` 結尾，機率很低。
  5. `by_stem` 以小寫檔名為 key，不同目錄的同名 `.prefab` 只留第一個（`or_insert`），也沒有報告衝突。
  - 模組文件記錄 2.06 有 8 / 917 個 dyeable 物品沒有解析到（`:28-29`），但沒有說明原因。上面任一點都可能是原因，也可能都不是。
- **可重現的輸入：** 在 `prefab_pac_paths_reads_length_prefixed_pac_strings`（`:1935-1954`）的 blob 裡加入 `s("character/model/a/B_UB.PAC")` 或 `s("character/model/a/b ub.pac")`。預期：兩者都不會出現在輸出裡。
- **嚴重度：** low　**確信度：** plausible（沒有比對 live 的 8 個未解析物品）　**來源：** 本次引入（`f33f21b`）。

### E-07 — 新入口繼承 A-01／A-02 的 abort

- **位置：** `src/c_abi/item_part_prefab.rs:154-155`（`PackMeta::parse(&std::fs::read(pamt_path)?, None)`）、`:571-583`（`open_prefab_archive`）、`:599-653`、`:662-706`、`:737-776`（三個會呼叫它的 `extern "C"` 入口）；每個 prefab 都經過 `paz::extract_file`（`paz.rs:716`：`vec![0u8; compressed_size]`）。
- **問題與根因：** 這些入口解析**呼叫端指定路徑**的 PAMT，然後依 PAMT 的宣告值抽取上千個 prefab。A-01（trie 遞迴造成 stack overflow）和 A-02（依宣告值配置記憶體，配置失敗就 abort）都會直接終止 C# 宿主行程，`catch_unwind` 擋不住。E 區塊本身沒有新的根因，但 `f33f21b` / `22db5f3` 新增了一個「從 C ABI 傳入任意 PAMT 路徑」的公開入口，而且一次會觸發上千次抽取。
- **可重現的輸入：** 見 A-01 / A-02 的構造方式，把那個 PAMT 的路徑當作 `prefab_pamt_path` 傳入。
- **嚴重度：** low（需要惡意或損壞的 PAMT）　**確信度：** plausible　**來源：** 入口是本次引入，根因既有（A 區塊）。

### E-08 — E 區塊的測試在 CI 上一個都不跑

- **位置：** `src/lib.rs:29-30,62-65,92-93`（三個核心模組和 `c_abi` 都是 `#[cfg(feature = "c_abi")]`）；`.github/workflows/ci.yml` 最後一步是 `cargo test --lib`，沒有 `--features c_abi`。
- **問題與根因：** 因為整個模組都被 feature gate 擋掉，連**不需要 install** 的合成測試在 CI 上也不會編譯：`dye_color_group_info::record_tests` 的 3 個測試、`palette_pabgh_parses_custom_six_byte_entries`、`prefab_pac_paths_reads_length_prefixed_pac_strings`、四個 `*_null_args`、三個 `*_empty_bytes_yields_empty_handle`、`resolve_dye_slot_count_source_constants_are_distinct`。CI 的 clippy 會用 `c_abi,python` 編譯這些測試，但不會執行。A-14 已經指出 checksum KAT 有同樣的問題；這裡補充的是 E 區塊的部分，其中包含本週期新增、正是用來釘住 2.06 新格式的測試。
- **可重現的情境：** 在 `record_tests::reads_uniform_and_mixed_records` 裡加一個必定失敗的 `assert!(false)`，推到 PR。預期：CI 綠燈。
- **嚴重度：** low　**確信度：** confirmed　**來源：** 既有問題（CI 設定）；受影響的測試有一半是本次引入。

### E-09 — `dye_slot_no` 的型別在兩個同名函式之間不一致

- **位置：** `src/c_abi/item_part_prefab.rs:1063`（`dye_slot_no: u8`）對照 `src/c_abi/part_prefab_dye_slot_info.rs:559`（`dye_slot_no: u32`，再用 `u8::try_from`，超出範圍回 `OUT_OF_RANGE`，`:573-578`）。`docs/dye-editor-scope.md:495` 的簽名沒有寫型別。
- **問題與根因：** 兩個函式名稱一樣都叫 `lookup_dye_slot_material`，參數順序也相近。C# 端如果照 slot-info 版本把 item 版本也宣告成 `uint`，在 Win64 ABI 下被呼叫端只讀低 8 位元：傳 256 會被當成 0，回 `OK` 和 slot 0 的材質，而不是 `OUT_OF_RANGE`。slot-info 版本的測試釘住了 300 → `OUT_OF_RANGE`（`part_prefab_dye_slot_info.rs:1581`），item 版本沒有對應的測試。
- **嚴重度：** nit　**確信度：** confirmed（簽名）／plausible（C# 端是否真的宣告錯）　**來源：** 本次引入（`f33f21b`）。

### E-10 — 重複 key 的處理不一致

- **位置：** `src/c_abi/dye_color_group_info.rs:55`、`part_prefab_dye_slot_info.rs:58`、`part_prefab_dye_texture_pallete_info.rs:48`（都是 `Entry::Vacant`，第一筆勝出）；`src/c_abi/item_part_prefab.rs:318-319`（`rows.into_iter().map(|e| (e.key, e)).collect()`，最後一筆勝出）；同檔 `:287-290`（`key_by_name`，第一筆勝出）。
- **問題與根因：** 表裡如果出現重複的 `PartPrefabKey`，slot-info handle 回傳第一列，item handle 的 `dye_slot_bits` / `dye_slot_material` / `prefab_names_by_item` 卻用最後一列。`item_part_prefab.rs:970-972` 說兩者 OR 起來會相等，這時就不成立。row key 是 `.pac` 路徑的 checksum，2.06 預期不會重複（`part_prefab_key_is_the_pac_path_checksum_live` 釘住 key = 路徑 checksum），但沒有任何測試釘住 key 唯一。
- **嚴重度：** nit　**確信度：** plausible　**來源：** 本次引入（`f33f21b`）。

### E-11 — Name-join handle 的 `merge_dlc` 仍會開 DLC PAMT

- **位置：** `src/c_abi/item_part_prefab.rs:724-725`（文件：「a name-join handle ignores it」）、`:759-766`（不論 resolver 是哪一種都會先 `open_prefab_archive`）、`:368`（之後才依 resolver 種類丟掉它）。
- **問題與根因：** 對 `load_from_{bytes,file}` 建出的 handle 傳一個不存在、或不含 `.prefab` 的 `dlc_prefab_pamt_path`，會回 `IO` 或 `NOT_FOUND`，merge 不會執行，和文件寫的「忽略」不符。
- **嚴重度：** nit　**確信度：** confirmed　**來源：** 本次引入（`22db5f3`）。

### E-12 — 回傳碼語意重疊

- **位置與內容：**
  - `src/c_abi/dye_color_group_info.rs:419-428`：`palette_knowledge_at` 的 `NOT_FOUND` 同時代表「group key 不存在」和「這格是 `00`，不需要解鎖」。呼叫端要先用 `palette_size` 才能分辨。文件有寫（`:398`），但「沒有條件」用錯誤碼表示，C# 端很容易把它當成錯誤。
  - `src/c_abi/item_part_prefab.rs:400-420,1001-1003`：「沒有這個物品」和「沒有任何 variant 符合 `tribe_gender`」都回 `NOT_FOUND`。
  - `src/c_abi/item_part_prefab.rs:958-966`：非 UTF-8 的 `tribe_gender` 回 `INVALID_PATH`，但它不是路徑。
  - `src/c_abi/dye_color_group_info.rs:376-381`：`position_for_rgb` 回傳第一個符合的位置。如果同一個色票裡有重複的 RGB（例如灰階區和某個淡色欄剛好相同），各自需要的 KnowledgeKey 不同，回傳哪一格會影響「是否已解鎖」的判斷。本回合沒有讀資料，**不知道 2.06 色票裡有沒有重複的 RGB**。
- **嚴重度：** nit　**確信度：** confirmed（程式碼）／plausible（重複 RGB 是否存在）　**來源：** `palette_knowledge_at` 與 `tribe_gender` 是本次引入（`3ca07dd`、`f33f21b`）；`position_for_rgb` 是既有問題（`d8c15cd`）。

### E-13 — 已知是常數的位元組沒有驗證

- **位置：** `src/part_prefab_dye_slot_info/mod.rs:335`（每個 slot 的 `0xFF` marker：「not stored」，也沒有檢查）、`:385`（row 的 `u8 pad` 直接跳過）、`:353-357`（extra layer 的 flag，0/1 以外的值也接受）；`src/part_prefab_dye_texture_pallete_info/mod.rs:174-175`（「One padding zero after the key — we don't enforce, just skip」）；`src/dye_color_group_info/mod.rs:284-285`（flag 與 4-byte 欄位直接跳過）；`src/part_prefab_dye_texture_pallete_info/mod.rs:106`（`pabgh.len() < needed` 只擋太短，不擋尾端多出來的位元組；同類的 `parse_pabgh` 會擋，`skill_info/pabgh.rs:42-52`）。
- **問題與根因：** 這些表沒有寫回路徑，所以不影響 roundtrip。但已知是常數的位元組正是最便宜的漂移偵測點。slot-info 和 pallete 有「整列剛好走完」的檢查（`:404`、`:213`），大部分漂移都能抓到；只有「長度不變、意義改變」的漂移（例如 marker 變成另一個 flag）會靜默通過。
- **嚴重度：** nit　**確信度：** confirmed　**來源：** 既有問題（`dd3ea2d`、`ebbb4f5`）。

### E-14 — 文件與程式碼脫節

全部是 confirmed、nit。

- `src/dye_color_group_info/mod.rs:9-30`：schema 仍寫 `u8 row_hash[4]`「per-row hash value」、`color_data[8 * color_count]`、「always 109」；`:117-119` 卻說那 4 個位元組是主題地圖 icon 的 `StringInfoKey`，`:32-41` 說紀錄是 22 bytes。`:94-98` 說 4-byte tail 是「per-row lightness key + `e1 ff ff`」（1.07 的解讀），和 `:34-36`（ConditionInfoKey）矛盾。`:205-210` 的註解仍是「8 * color_count bytes」。`:5`、`:158` 寫「10 rows in 1.07」、「109 records in 1.07」。既有問題，`5a77bbd` / `3ca07dd` 加了新段落但沒有改舊段落。
- `src/dye_color_group_info/mod.rs:549-619`（`#[ignore]` 的 `dye_gradient_vs_slot103_rgbs`）：固定以 8-byte stride 走訪、也不做 BGRA 交換，在 2.06 上會印出錯的位置。既有問題，2.06 起失效。
- `src/part_prefab_dye_slot_info/mod.rs:9-13`：列數停在 1.12（968）；`:161-162`：`key` 的說明是「sits in `iteminfo.pabgb` — not yet bridged」，實際上 key 是 `.pac` 路徑的 checksum。`:87-89`：「For the v1 dye editor we only consume `(key, prefab_name, slot_count)`」。既有問題。
- `src/c_abi/part_prefab_dye_slot_info.rs:28-31`：「That linkage is the next-session blocker」，但 `item_part_prefab` 已經存在。既有問題。
- `src/c_abi/part_prefab_dye_slot_info.rs:394-395,745-746`、`docs/dye-editor-scope.md:615`：`_mask_full` 寫「3 on 1.07-2.00」，但 `mask` 固定是 `[u8; 12]`。已由 C-15 記錄，這裡不重複計分。
- `src/part_prefab_dye_texture_pallete_info/mod.rs:5-6,40`、`src/c_abi/part_prefab_dye_texture_pallete_info.rs:5-7,276,428-430`、`mod.rs:249-253`：仍寫 `key=0` 有 2 個 sub、icon path 退回 texture path；1.13 起 `key=0` 有 3 個 sub（測試 `:358-372`、`c_abi/...:520-530` 已更新）。既有問題。
- `docs/dye-editor-scope.md:596`（「11 in 1.07」）、`:608`（「1,105 in 1.07」）、`:609`（見 E-01）。既有問題。
- `docs/dye-editor-scope.md:620-622`：錯誤碼清單只列 `NULL_ARG / NOT_FOUND / OUT_OF_RANGE / BUFFER_TOO_SMALL / PANIC`，漏了 E 區塊實際會回的 `IO`、`INVALID_PATH`、`BODY_PARSE`、`UNSUPPORTED_VERSION`。
- **符號對照：** E 區塊共有 59 個 `extern "C"` 符號（dye 11、slot-info 22、pallete 11、item 15）。`docs/c-abi-surface.md` 只完整寫出 `crimson_item_part_prefab_merge_dlc_from_bytes`，其餘交給模組地圖的縮寫和 `dye-editor-scope.md`。`dye-editor-scope.md` 涵蓋了 58 個（部分用 `_lookup_sub_icon_path` 這類縮寫），**`crimson_item_part_prefab_resolved_item_count`（`item_part_prefab.rs:831`）在 repo 的任何 `.md` 都找不到**。`lookup_slot_extra_layer_{count,material,mask,flag}` 只出現在 `c-abi-surface.md:470` 的縮寫裡，`dye-editor-scope.md` 的 slot-info 表格（`:605-618`）沒有列。

### E-15 — FFI 衛生

全部是 confirmed、nit。

- `#[repr(C)]` 標在四個內含 `HashMap` / `Vec` 的 opaque handle 上：`c_abi/dye_color_group_info.rs:31`、`part_prefab_dye_slot_info.rs:45`、`part_prefab_dye_texture_pallete_info.rs:34`、`item_part_prefab.rs:90`。沒有實際效果，還會讓人誤以為 C 端可以讀它的佈局（同 A-19）。
- 私有的 safe `fn` 直接解參考呼叫端給的 raw pointer：`item_part_prefab.rs:571-574`（`open_prefab_archive`）、`:958-963`（`tribe_gender_arg`）、`part_prefab_dye_texture_pallete_info.rs:216-249`（`lookup_sub_string`）。目前只被 `unsafe extern` 入口呼叫，所以不是對外的 unsoundness，但違反了「解參考 raw pointer 的函式要標 `unsafe`」的慣例。clippy 的 `not_unsafe_ptr_arg_deref` 只檢查 `pub` 函式，所以抓不到。
- 錯誤路徑上沒有清空 out-param：`part_prefab_dye_slot_info.rs:422-435,764-781`（`_mask_full` 系列在 `NOT_FOUND` / `OUT_OF_RANGE` 時 `*required` 保持呼叫端原本的值；同檔其他 sized-buffer getter 都有先寫 0）；`:337-350,378-391,467-481,726-742`（3-byte out 在錯誤時不清空）；`*_entry_count` 與 `item_part_prefab.rs:838-843` 沒有先清空（只有 panic 時有差別）。
- 重複的 helper：`write_str_to_buf` 有三份（`dye_color_group_info.rs:476`、`part_prefab_dye_slot_info.rs:863`、`part_prefab_dye_texture_pallete_info.rs:408`），加上 `item_part_prefab.rs:942-950,1086-1094` 兩處 inline 版本；`read_cstring` 有兩份；`palette_entry_ranges`（`part_prefab_dye_texture_pallete_info/mod.rs:127-142`）幾乎是 `skill_info::entry_ranges` 的翻版。
- 記憶體：三個表 handle 都把每一列存兩份（`by_key` 存 clone，`entries` 存原物件，例如 `part_prefab_dye_slot_info.rs:57-62`）；item handle 為了 merge 保留整份 stringinfo 對照表和 0009 的 `PackMeta`（`item_part_prefab.rs:104-108`），handle 存活期間一直佔用。

### E-16 — 測試小問題

全部是 confirmed、nit，本次引入（`ef31f24`、`87b042e`、`f33f21b`）。

- `src/c_abi/part_prefab_dye_slot_info.rs:1471-1475`：`used` 用 `1 << n` 計算，但 `n < 12` 的 assert 在下一行（`:1476-1481`）。如果某個 `mat_indices` 是 32..254，debug build 會先因位移溢位而 panic，看不到原本要顯示的訊息。
- `src/c_abi/item_part_prefab.rs:1829-1832`、`:2205-2208`：`_dyeSlotNo` 來自存檔（`i64`），`fold(0u32, |a, n| a | (1 << n))` 遇到 ≥ 32 的值會溢位 panic，也是在 assert 之前。
- `src/c_abi/item_part_prefab.rs:2051-2067`（`part_prefab_key_is_the_pac_path_checksum_live`）：這個不變式只需要 0008 的表，但測試透過 `live_prefab_join()` 取資料，所以 0009 PAMT 不存在、或整個 prefab join 失敗時都會 skip。另外它只檢查 `rows.len() > 1000`，不和 header 列數比對（需要搭配 `part_prefab_dye_slot_info_lossy_live` 的「每列都要解析成功」才完整）。
- `src/c_abi/part_prefab_dye_slot_info.rs:1423-1431`：`DYE_SLOT_PINS` 的說明重複寫了兩次 tuple 欄位。

---

## 跨區塊關注點逐項結果

| 主題 | 結果 |
|---|---|
| 不可信輸入配置上限 | E 區塊自己的配置都有上限：`color_count ≤ 1024`（`dye_color_group_info/mod.rs:295`）、`slot_count ≤ 64`（`part_prefab_dye_slot_info/mod.rs:390`）、`extra_count ≤ 8`（`:344`）、`sub_count ≤ 16`（`part_prefab_dye_texture_pallete_info/mod.rs:181`）、pallete header 的 count 是 `u16`、`prefab_pac_paths` 的字串長度 ≤ 1024、`legacy_pair` 的 capacity 以輸入長度為界。超過上限的配置只會透過 `paz::extract_file` 發生（E-07，根因在 A-02）。 |
| Panic 面與 abort | E 區塊自己的執行路徑上沒有可觸發的 panic（見排除說明 4）。abort 只會經由 E-07 從 A 區塊繼承。測試裡有幾處位移溢位（E-16）。 |
| Roundtrip 對稱性 | 不適用：E 區塊的三張表都只讀不寫，也沒有 Python 寫回路徑。`legacy_pair` 重建屬於 B 區塊。常數位元組沒有驗證見 E-13。 |
| 版本漂移明確失敗 | 不符合：E-02、E-03、E-04、E-05。 |
| Feature gate 組合 | 四個核心與 C ABI 模組都只在 `c_abi` 下編譯（`lib.rs:29-30,62-65,92-93`）；`default` 與 `python` 都不會編譯它們，也沒有其他模組在那兩種組合下引用它們。`c_abi` 單獨編譯時會套用 `cfg_attr(allow(dead_code))`，但 `c_abi,python` 的 strict clippy 仍會檢查它們。本回合沒有發現被 `allow(dead_code)` 掩蓋的死碼（`parse_palette_after_key` 是 `#[cfg(test)]`）。CI 不執行這些測試見 E-08。 |
| `docs/c-abi-surface.md` 一致性 | 見 E-01（`resolve_dye_slot_count` 的描述）與 E-14（`resolved_item_count` 沒有文件；`c-abi-surface.md` 沒有列出 E 區塊的完整符號）。 |

---

## 排除說明（查證後沒有問題的審計重點與待查事項）

計畫書的「掃描時注意到的待查事項」**沒有任何一條分配給 (E)**，所以這裡只列審計重點與跨區塊項目中查證後沒有問題的部分。

1. **審計重點 2：1,703 / 1,703 列 key 等於路徑 checksum 的不變式有測試釘住。** `part_prefab_key_is_the_pac_path_checksum_live`（`item_part_prefab.rs:2051-2067`）逐列檢查最後一個 `tail_name` 以 `.pac` 結尾，且 `part_prefab_key_for_pac(tail) == key`，另外還釘了一個已知值 `0xe3634591`。搭配 `part_prefab_dye_slot_info_lossy_live`（`part_prefab_dye_slot_info/mod.rs:528-533`）要求「每一個 header 列都要解析成功」，任何一列打破這個規則都會被抓到。只有 E-16 提到的「不必要地依賴 0009」這個小缺點。
2. **審計重點 3：dye slot 編號與 12-byte mask 的位元對應一致。** `mask_bits`（`part_prefab_dye_slot_info/mod.rs:235-240`）是 bit `n` ↔ `mask[n]`；`active_channels` 用 `mask.get(n)` 判斷，`n ≥ 12` 也安全（`:261-268`）；`channel_dye_slot_bits` 有 `n < 32` 的保護（`:199-205`）。slot-info handle（`c_abi/part_prefab_dye_slot_info.rs:540`）和 item handle（`item_part_prefab.rs:425`）都呼叫同一個 `PartPrefabDyeSlotInfoEntry::dye_slot_bits`，所以兩者一致（重複 key 的例外見 E-10）。存檔端的對應有 `dye_slot_no_is_a_prefab_dye_slot_in_the_1_10_fixture` 與 `..._via_prefabs_...` 兩個 fixture 測試釘住。另外，計畫書把 `lookup_dye_slot_bits` 歸在 C 區塊的 `iteminfo.rs`，實際上 `src/c_abi/iteminfo.rs` 裡沒有任何 dye slot 或 mask 的程式碼，只有 `is_dyeable` 旗標（C 報告第 237 行也有同樣的說明）。
3. **審計重點 4：combined handle 的生命週期沒有問題。** `CrimsonItemPartPrefabHandle` 自己擁有所有資料（`rows`、`strings`、`PackMeta`），不借用其他 handle 的任何東西，所以 free 的順序無關。`resolve_dye_slot_count` 只在呼叫期間讀兩個 handle。`merge_dlc` 需要 `*mut`，文件也寫明不能和讀取同時進行（`item_part_prefab.rs:725-726`）。重複 free 是 UB，但這和 repo 裡所有 `Box::into_raw` handle 的契約相同，不是 E 區塊特有的問題。
4. **審計重點 1（健壯性的部分）：`.prefab` 掃描不會 panic。** `prefab_pac_paths` 的 `try_into().unwrap()` 作用在 `get(i..i + 4)` 回傳的 4-byte 切片上，不可能失敗；其餘都用 `get`；64 位元下 `i + 4 + len` 不會溢位。其他看起來是直接索引的地方也都安全：dye 色票的 `body[o + 3]` 在兩條路徑上都保證 `o + 4 ≤ to ≤ body.len()`（逐筆走訪要求 `body.get(at + 4..to)` 存在；等長切分要求 `span / count ≥ 4`）；`insert_item` 的 `self.rows[k]` 只會拿到 `pac_keys` 過濾過、或從同一份 `rows` 建出的 key；`PrefabArchive::pac_paths` 的 `directories[di].files[fi]` 是從同一個 `PackMeta` 列舉出來的索引。路徑正規化的缺口另見 E-06。
5. **`mask` 3-byte partial read 的文件一致性**（C 區塊審計重點 4 的 E 部分）：`lookup_slot_mask` 的 rustdoc、`dye-editor-scope.md:614` 和 CLAUDE.md 都清楚寫了「2.01 起意義錯誤，改用 `_full` / `_active_channels` / `_dye_slot_bits`」，`c_abi_part_prefab_dye_slot_numbers_live` 也釘住了 `0xc7bbaada` 的 3-byte 讀法是全零。C-15 已經記錄的「3 on 1.07-2.00」不重複計分。
6. **`legacy_pair` 接線：** 三個核心解析器都在進入點先呼叫 `legacy_pair`。key 寬度與重建後的 header 形狀相符：dye 與 slot-info 是 `(u16 count, u32 key)`，接 `parse_pabgh`；pallete 是 `(u16, u16)`，接 `parse_palette_pabgh`，`read_row_head(body, 2)`。`item_part_prefab` 的 iteminfo 與 stringinfo 走 body-only 的 2.06 直讀路徑，這是 C 區塊的設計，DLC iteminfo 只在 E 的測試裡逐列消耗過（已由 C 報告第 65 行記錄）。
7. **DLC merge 的原子性：** `merge_dlc` 在修改 handle 前先完成所有解析與 join（`item_part_prefab.rs:364-375`），commit 階段（`:377-393`）只有不會失敗的 `HashMap` / `Vec` 操作，`dlc_count` 也是在 commit 才遞增。`c_abi_item_part_prefab_without_dlc_is_unchanged_live` 釘住了失敗後 handle 不變。錯誤會被吞掉的部分見 E-05。
