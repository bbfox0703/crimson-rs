# Code audit — A. 封存容器與加密（2026-11-28）

依 [`code-audit-2026-11-28-plan.md`](code-audit-2026-11-28-plan.md) 的 A 區塊做靜態審計。**沒有修改任何程式碼**，沒有執行 crimson-rs 的任何程式或測試，也沒有碰 PAPGT trailer（只讀了 `papgt.rs` 如何原樣保存它）。

- **基準：** `dev` @ `e465942`。下文的行號都以這個 commit 為準。
- **範圍：** `src/binary/{primitives,types,arrays,mod,pamt,papgt,paz,trie,paloc,paver}.rs`、`src/crypto/{chacha20,checksum}.rs`、`src/c_abi/{paz,paloc,paver,checksum}.rs`。為了判斷錯誤會傳到哪裡，也看了直接呼叫端：`src/c_abi/item_part_prefab.rs:145-186`（批次抽 `.prefab`）、`src/python.rs`（PAMT/PAPGT/PAZ 的 Python 入口與 `PackGroupBuilder` 參數）、`python/crimson_rs/{create_pack,pack_mod}.py`、`src/lib.rs:1-15`（feature gate）、`.github/workflows/ci.yml`、`Cargo.toml` / `Cargo.lock`（依賴版本與 panic 策略）。
- **方法：** 逐行閱讀原始碼，用 `git blame` 判定來源歸屬，並對照 `docs/c-abi-surface.md`、CLAUDE.md 與計畫書。
- **這次沒做到的資料比對：**
  1. 我原本打算用一段**唯讀**、不經過 crimson-rs 的 Python（從 stdin 執行、不落地）掃 live install 全部 `0.pamt` 的中繼資料，量化 A-03、A-05、A-06、A-08 在真實資料裡是否出現（例如：有沒有 zlib 項目、有沒有未知的 compression／crypto nibble、有沒有檔名雜湊讓 ChaCha20 計數器溢位的加密檔）。這個指令在本 session 需要額外核准而沒有執行，所以這幾條的「真實資料是否命中」都列在「未能驗證」，確信度一律是 plausible。
  2. 計畫書提到的依賴行為（`lz4_flex 0.11.6` 的 `min_uncompressed_size` 語意、`chacha20 0.10.0` / `cipher 0.5.1` 的計數器上限）我沒辦法讀到 `~/.cargo/registry` 的原始碼（目錄在允許範圍外）。凡是靠這些語意推出的後果，都標成 plausible，並在該條寫明依據。

### 確信度與來源歸屬的定義

沿用 B 區塊報告的定義：

- **confirmed**：只用原始碼或文件文字就能確定的事實，例如「沒有這個檢查」、「沒有這組測試」、「文件和程式碼不一致」。
- **plausible**：行為上的後果是靠追程式路徑或依賴的已知語意推出來的，**沒有用位元組或實際重現驗證過**。依 CLAUDE.md 與計畫書的規定，這類一律標 plausible，即使程式路徑本身是確定性的。
- **本次引入**：引入的 commit 在 2.06 移植週期內，也就是 `95fa25f`（2026-11-27）到基準 `e465942` 之間。A 區塊在這段期間只有 `95fa25f` 動過 `binary/mod.rs`（+1）、`binary/types.rs`（`HeaderName`）、`binary/paver.rs` 和 `c_abi/paver.rs`（常數與文件），所以下面幾乎全部是既有問題。
- **既有問題**：引入時間早於 `95fa25f`。主要的來源 commit：`8623be0`（2026-03-28，PAZ/PAMT/PAPGT 打包與 checksum）、`6d9184a`（2026-03-28，paz overlay）、`b038c2d`（2026-04-23，`CArray` 上限）、`6b75402`（2026-05-13，C ABI PAZ 抽取）、`61c2f52`（2026-05-23，paver）、`1f32ec2`（2026-11-01，2.04 的 `CFlaggedArray`）。

---

## 摘要

| # | 標題 | 嚴重度 | 確信度 | 來源 |
|---|---|---|---|---|
| A-01 | Trie `get_string` 無界遞迴：舊編碼可自我循環，新編碼可深鏈，都會 stack overflow 讓整個行程 abort | medium | plausible | 既有問題 |
| A-02 | 依 PAMT 宣告值配置記憶體（`dir_count`、`file_count`、`compressed_size`、`uncompressed_size`），可達 4–80 GiB，配置失敗直接 abort | medium | plausible | 既有問題 |
| A-03 | `decompress` 不驗證輸出長度等於 PAMT 宣告值，zlib 也沒有上限（解壓縮炸彈） | medium | confirmed（缺檢查）／plausible（後果） | 既有問題 |
| A-04 | `PackGroupBuilder` 用呼叫端的 `encrypt_info` 加密，PAMT 表頭卻寫死 `[2,14,97]`：ChaCha20 打包的檔案用自己的 PAMT 解不回來 | medium | confirmed（程式碼）／plausible（後果） | 既有問題 |
| A-05 | 未知的 compression（5–15）與 crypto（4–15）nibble 被當成「不壓縮／不加密」，版本漂移不會明確失敗 | low | confirmed（程式碼）／plausible（後果） | 既有問題 |
| A-06 | ChaCha20 用 32 位元區塊計數器並 `seek` 到檔名雜湊，計數器接近上限的大檔會在 `apply_keystream` panic | low | plausible | 既有問題 |
| A-07 | `crimson_paz_extract_file` 在抽出 0 位元組、`out_buf` 為 null 時仍呼叫 `copy_nonoverlapping`（UB；debug build 會 abort） | low | plausible | 既有問題 |
| A-08 | PAMT 的 `chunk_offset + compressed_size` 是 u32 加法：debug panic，release 溢位後繞過邊界檢查 | low | confirmed（程式碼）／plausible（後果） | 既有問題 |
| A-09 | PAPGT 舊版佈局的 `i32` 名稱長度為負時，`check_remaining` 的加法溢位，最後 panic | low | plausible | 既有問題 |
| A-10 | Roundtrip 不對稱：`COptional` flag ≥ 2、`CFlaggedArray` 的 `01`+count 0、paver 未知尾段、PAPGT 舊版尾端多餘位元組 | low | confirmed（程式碼）／plausible（實際出現） | 既有問題 |
| A-11 | `CArray` 的上限以「剩餘位元組數」計，不是「剩餘位元組 / 元素最小大小」，配置可達剩餘位元組 × `size_of::<T>()` | low | plausible | 既有問題 |
| A-12 | DDS per-mip 解碼器：`mip_count`／寬高沒有上限，算術在 release 下會 wrap，迴圈可達 2³² 次 | low | plausible | 既有問題 |
| A-13 | 版本閘門只有常數和文件公式，沒有 Rust 或 C ABI 函式實作「成對比較 + patch 下限」，也沒有測試釘住公式 | low | confirmed | 既有問題（常數在本次更新） |
| A-14 | A 區塊的關鍵測試在 CI 上不跑：CI 是 `cargo test --lib`（不含 `c_abi`），checksum KAT 只在 `c_abi` 測試裡；ChaCha20 沒有 KAT；解壓縮沒有宣告值不符的測試；builder 測試沒有用到加密 | low | confirmed | 既有問題 |
| A-15 | Python-only builder 的截斷：PAPGT `entry_count` u8 wrap、trie 片段長度 `as u8`、`PackGroupBuilder` 的 u32/u16 截斷、`add_entry` 不拒絕內含 NUL 的名稱 | low | confirmed（程式碼）／plausible（後果） | 既有問題 |
| A-16 | PALOC handle：重複 key 時 lookup 與列舉不一致；非 UTF-8 字串每次載入都 `Box::leak` | low | plausible | 既有問題 |
| A-17 | Paver 的 DLC 尾段讀不懂時退回 10 位元組讀法，`build` 變成 DLC 數量；`BinaryRead for Paver` 在 offset 越界時 panic | nit | plausible | 既有問題 |
| A-18 | 文件與程式碼脫節：缺 `.paz` 回 `BODY_PARSE`（文件說 `IO`）、worldmap SDF 的舊說明、`c-abi-surface.md` 沒列 `crimson_paloc_*`、`pamt.rs` 的 checksum 註解、`hashlittle2` 命名等 | nit | confirmed | 既有問題 |
| A-19 | Feature gate／lint 小問題：`papgt.rs` 對有用到的 import 標 `allow(unused_imports)`、`Compression::Partial` 從未建構、`CrimsonPalocHandle` 標 `#[repr(C)]` | nit | confirmed | 既有問題 |

沒有 critical 或 high 的 finding。A 區塊的問題集中在「**不可信的長度與偏移直接驅動配置、遞迴或算術**」。真實遊戲檔目前都能正常讀（live roundtrip 測試在維護者機器上通過），所以 A-01、A-02、A-03、A-09、A-11、A-12 的觸發條件都是壞檔或惡意的 mod 檔。但 C ABI 會解析呼叫端指定路徑的 PAMT，而 Jenkins checksum 不是密碼學雜湊，攻擊者可以直接重算（`to_bytes_with_checksum` 就是在做這件事），所以 checksum 擋不住這幾條。其中 A-01 和 A-02 會讓 C# 宿主行程直接 abort，`catch_unwind` 救不回來。

---

## Findings

### A-01 — Trie `get_string` 無界遞迴，可 stack overflow

- **位置：** `src/binary/trie.rs:96-135`（`get_string`，遞迴在 `:130`，快取在遞迴**之後**才寫入 `:133`）、`:59-62`（`AbsoluteParent` 的 parent 是任意 `i32`）、`:63-91`（`RelativeParent`）。呼叫端：`src/binary/pamt.rs:301`（目錄名）、`:361`（檔名）。
- **問題與根因：**
  1. **≤ 2.03 的 `AbsoluteParent` 編碼**：parent 是檔案裡的任意 `i32`，程式沒有檢查「parent 必須在自己之前」，也沒有偵測循環。快取要等遞迴回來才寫入，所以擋不住循環。offset 0 的節點只要把 parent 寫成 0，`get_string(0)` 就會無限遞迴。
  2. **2.04+ 的 `RelativeParent` 編碼**：距離必須 ≥ 1 且 ≤ offset（`:85-91`），所以不會循環。但深度沒有上限：每個節點最少 2 個位元組（1 byte ULEB128 + 長度 0），N 位元組的緩衝區可以構成 N/2 層的鏈。
  3. Rust 的 stack overflow 不是 panic，是直接終止行程。C ABI 入口的 `catch_unwind`（例如 `c_abi/paz.rs:160,224`）擋不住，C# 宿主會整個當掉。
  4. 選哪種編碼由「PAMT 有沒有副檔名表」決定（`pamt.rs:264-289`），所以攻擊者只要省略副檔名表，就能選到可循環的舊編碼。
- **可重現的情境：** 構造一個 PAMT：表頭 `count = 0`；目錄 trie 是 5 個位元組 `00 00 00 00 00`（parent = 0、長度 0）；檔名 trie 為空；`dir_count = 1`，`name_offset = 0`；`file_count = 0`；沒有副檔名表。用 `checksum::calculate_checksum` 重算表頭 checksum。把它交給 `crimson_paz_list_dir(pamt_path, "x", …)` 或 Python 的 `parse_pamt_bytes`。預期結果：`PackMeta::parse` → `dir_trie.get_string(0)` 無限遞迴 → stack overflow → 行程終止。目錄名 checksum 的驗證（`pamt.rs:304`）在 `get_string` **之後**，擋不到。
- **嚴重度：** medium（需要構造的檔案，但結果是無法攔截的行程終止）。
- **確信度：** plausible（程式路徑已追過，沒有實際執行）。
- **來源：** 既有問題（`8623be0`；`RelativeParent` 分支來自 `1f32ec2`）。
- **建議方向（不在本次修改）：** 改成迭代走訪；`AbsoluteParent` 要求 `parent < offset`；加深度上限（例如 256），或在進入遞迴前先插入「進行中」標記。

### A-02 — 依宣告值配置記憶體，配置失敗會 abort

- **位置：**
  - `src/binary/pamt.rs:246-251`（`Vec::with_capacity(dir_count)`，元素 16 B）、`:255-260`（`Vec::with_capacity(file_count)`，元素 20 B）。兩個 count 都是未檢查的 u32。
  - `src/binary/paz.rs:716`（`vec![0u8; compressed_size]`，上限 4 GiB）。
  - `src/binary/paz.rs:35`（`lz4_flex::block::decompress(data, uncompressed_size)`）、`:40`（zlib `Vec::with_capacity(uncompressed_size)`）、`:124-128`（partial 策略 2 的 `decompress_with_dict(…, uncompressed_size - 128, …)`）。
- **問題與根因：**
  1. `dir_count` / `file_count = 0xFFFFFFFF` 時，`with_capacity` 要 64 GiB / 80 GiB。這兩個值沒超過 `isize::MAX`，所以不會走「capacity overflow」那條可以攔截的 panic，而是真的去配置。Windows 不 overcommit，配置會失敗，`std` 預設的 `handle_alloc_error` 會**終止行程**，`catch_unwind` 一樣擋不住。正確做法應該像 `CArray` 一樣先跟剩餘位元組比（`count * 16 <= remaining`）。
  2. `compressed_size` 只跟 PAMT 自己宣告的 `chunk.size` 比（`pamt.rs:351`），沒有跟 `.paz` 檔的實際大小比。所以 `extract_file` 會先配置並清零最多 4 GiB，`read_exact` 才失敗。
  3. LZ4 / zlib / partial 的輸出緩衝區都以 `uncompressed_size` 預先配置。一個 200 位元組的條目，只要宣告 `uncompressed_size = 0xFFFFFFFF`，就會觸發一次 4 GiB 配置。依 `lz4_flex` 0.11 `block::decompress` 的已知實作（先依 `min_uncompressed_size` 配置輸出），LZ4 路徑也是這樣，但我沒讀到 crate 原始碼。
- **可重現的情境：**
  - PAMT：`file_count` 寫成 `0xFFFFFFFF`，其餘合法，重算 checksum，呼叫 `crimson_paz_list_dir`。預期在 `pamt.rs:257` 嘗試配置 80 GiB → abort。
  - PAMT：某檔 `flags = 0x02`（LZ4）、`compressed_size = 16`、`uncompressed_size = 0xFFFFFFFF`，呼叫 `crimson_paz_extract_file`。預期在 `paz.rs:35` 配置約 4 GiB（成功就是瞬間吃掉 4 GiB，失敗就 abort）。
- **緩解因素：** 真實遊戲的 PAMT 值都合理，live roundtrip 測試有涵蓋。
- **嚴重度：** medium。
- **確信度：** plausible。
- **來源：** 既有問題（`8623be0`、`6b75402`）。
- **建議方向：** count 先跟剩餘位元組除以元素大小比較；`compressed_size` 先跟 `.paz` 的 `metadata().len()` 比；`uncompressed_size` 設合理上限，或改用串流加 `take()`。

### A-03 — 解壓縮不驗證輸出長度，zlib 沒有上限

- **位置：** `src/binary/paz.rs:28-49`（`decompress`），特別是 `:37-43`（zlib `read_to_end`）和 `:35-36`（LZ4）；呼叫端 `:741-745`。對照組：`paloc.rs:92-97`、`paz.rs:130`、`:208`、`:219` 都**有**檢查長度。
- **問題與根因：**
  1. **zlib**：`ZlibDecoder::read_to_end` 會一直讀到串流結束，沒有用 `uncompressed_size` 當上限，也不比較最後的長度。所以：(a) 宣告值偏小時，輸出比宣告值大，而且沒有上限，幾 MB 的壓縮資料可以解出幾 GB（解壓縮炸彈），配置失敗就 abort；(b) 宣告值和實際不符時一律靜默接受。
  2. **LZ4**：`lz4_flex::block::decompress` 的第二個參數叫 `min_uncompressed_size`。依它的已知語意，串流比宣告值短時，函式會回傳較短的 `Vec`，不會報錯（未讀 crate 原始碼）。`decompress` 之後沒有長度檢查，所以截斷的 LZ4 區塊會被當成成功。
  3. **`Compression::None`**（非 partial）：不檢查 `compressed_size == uncompressed_size`。
  4. 計畫書審計重點 2 問的就是「宣告值不實時怎麼處理」。目前的答案是：除了 partial 和 PALOC 容器以外，一律靜默接受。
  5. C ABI `crimson_paz_extract_file` 回報給呼叫端的大小是實際長度（`c_abi/paz.rs:215-216`），不是 PAMT 宣告值。所以 C# 端也看不出兩者不一致。
- **可重現的情境：** 一個 `flags = 0x03`（zlib）的條目，內容是 `zlib.compress(b"\0" * 2**31)`（約 2 MB），`uncompressed_size = 16`。預期 `extract_file` 嘗試產生 2 GiB 的輸出，不會報錯（或 OOM abort）。換成宣告 `uncompressed_size = 2**31 + 1` 的正常檔案：預期回 OK，長度少 1。
- **嚴重度：** medium。
- **確信度：** confirmed（原始碼裡沒有長度檢查）／plausible（實際後果與 lz4_flex 的短輸出語意）。
- **來源：** 既有問題（`8623be0`）。
- **建議方向：** zlib 改成 `decoder.take(uncompressed_size as u64 + 1)`，結束後比較長度；LZ4 補上 `out.len() == uncompressed_size`；`None` 比較兩個 size。

### A-04 — `PackGroupBuilder` 加密用的 `encrypt_info` 和寫進 PAMT 的不一致

- **位置：** `src/binary/paz.rs:440-458`（`new` 收 `encrypt_info`）、`:470-476`（`add_file` 用 `self.encrypt_info` 加密）、`:663-671`（`build_pamt` 把表頭寫死成 `unknown0: 50, encrypt_info: [2, 14, 97]`）。抽取端：`paz.rs:727`、`c_abi/paz.rs:205`、`python.rs:1021,1083` 都用 **PAMT 表頭**的 `encrypt_info` 解密。對外介面：`python.rs:871-888`（預設 `encrypt_info=[0,0,0]`）、`create_pack.py:15-30`、`pack_mod.py:16-51`、`.pyi:1071-1072`。
- **問題與根因：** ChaCha20 的金鑰會 XOR 上 `k = ei[0] ^ ei[1] ^ ei[2]`（`chacha20.rs:32-36`）。builder 用呼叫端給的 `ei` 加密，PAMT 表頭卻永遠寫 `[2,14,97]`（k = 109）。只要呼叫端的 XOR 不是 109（包括預設的 `[0,0,0]`，k = 0），用這份 PAMT 抽檔就會解出亂碼。`ChaCha20 + Compression::None` 的情況下沒有任何錯誤，會直接交出亂碼；壓縮的情況下會在解壓縮時失敗。
- **可重現的情境：** `PackGroupBuilder(out, compression=0, crypto=3)`（`encrypt_info` 用預設值），`add_file("d","a.txt",b"hello")`，`finish()`，再用 `extract_file` 或 `crimson_paz_extract_file` 抽 `d/a.txt`。預期回 OK，但內容不是 `hello`。
- **緩解因素：** 預設 `crypto=0`；現有的 builder 測試全部是 `CryptoType::None`（見 A-14），所以從來沒踩到。
- **嚴重度：** medium（Python 打包 API 的正確性問題；依 CLAUDE.md，2.04+ 上 `pack_mod` 的輸出本來就不保證能載入，這條只講「自己的 PAMT 和自己的 PAZ 互相矛盾」）。
- **確信度：** confirmed（程式碼裡兩個值不同源）／plausible（亂碼的後果沒有實際執行）。
- **來源：** 既有問題（`8623be0`；表頭常數在 `6d9184a`）。

### A-05 — 未知的 compression / crypto nibble 被當成「沒有」

- **位置：** `src/binary/pamt.rs:98-123`（`from_raw`：compression `5..=15 → None`、crypto `4..=15 → None`）；`paz.rs:720-745`。
- **問題與根因：** 下個 patch 只要引進新的壓縮或加密代碼，`extract_file` 就會把仍是密文或壓縮過的位元組當成明文交出去，回 OK。這違反計畫書「跨區塊關注點／版本漂移：要明確失敗」。目前只有 `QuickLz`（4）和 ICE / AES（1 / 2）會回 `Unsupported`。
- **可重現的情境：** 把某個 PAMT 檔案條目的 `flags` 改成 `0x05`（並重算 checksum），呼叫 `crimson_paz_extract_file`。預期回 OK，內容是原始 chunk 位元組。
- **緩解因素：** 下游的表解析器多半會因為內容是亂碼而失敗，但錯誤訊息會指向解析器，不會指向 PAZ。
- **嚴重度：** low。
- **確信度：** confirmed（程式碼）／plausible（後果）。
- **來源：** 既有問題（`8623be0`）。

### A-06 — ChaCha20 的 32 位元計數器可能用盡而 panic

- **位置：** `src/crypto/chacha20.rs:20-49`，特別是 `:39-47`（初始計數器 = 檔名的 Jenkins 雜湊，`seek(counter * 64)` 後 `apply_keystream`）。依賴：`chacha20 0.10.0`、`cipher 0.5.1`（`Cargo.lock:54-67`）。
- **問題與根因：** `chacha20_crate::ChaCha20` 是 IETF 變體，區塊計數器是 32 位元。依 RustCrypto `cipher` 的已知行為，`apply_keystream` 是 `try_apply_keystream(..).unwrap()`，要求的金鑰串流超過計數器剩餘範圍時會 panic；`seek` 超出範圍時同樣會 panic。我沒讀到這兩個版本的原始碼，所以這部分是推論。檔名雜湊均勻分布在 2³² 內，所以一個 S 位元組的加密檔，在計數器落在最後 ⌈S/64⌉ 個區塊內時會 panic（機率約 S/64/2³²）。整個 install 的期望命中數約為 Σ(加密檔區塊數) / 2³²，在加密資料量達到數十 GB 時並不小。
  - 遊戲（或 Python 參考實作）在計數器溢位時是 wrap 還是進位到 nonce，目前不知道。不管是哪一種，Rust 這邊都只會 panic，不會產生正確的明文。
  - C ABI 會攔下 panic 回 `PANIC`。但 `item_part_prefab` 是在 `catch_unwind` 內**批次**抽 `.prefab`（`c_abi/item_part_prefab.rs:174-185`）：`.ok()?` 攔不住 panic，所以只要有一個檔名命中，整個 handle 載入都會失敗。
- **可重現的情境：** 找一個（或用 `PackGroupBuilder` 構造一個）檔名雜湊 ≥ `0xFFFF_FF00`、大小 > 16 KiB 的 ChaCha20 條目，抽取它。預期在 `chacha20.rs:47`（或 `:44`）panic。
- **未能驗證：** 真實 install 有沒有命中的檔案（見開頭「這次沒做到的資料比對」）。
- **嚴重度：** low（真實資料是否命中未知；命中時是固定那個檔案永遠抽不出來）。
- **確信度：** plausible。
- **來源：** 既有問題（`8623be0`）。
- **建議方向：** 用 `try_seek` / `try_apply_keystream` 回錯誤；確認遊戲的溢位語意後，再決定要不要改用 64 位元計數器（`ChaCha20Legacy`）或手動分段。

### A-07 — 0 位元組抽取時對 null 指標呼叫 `copy_nonoverlapping`

- **位置：** `src/c_abi/paz.rs:215-222`。
- **問題與根因：** 第一次呼叫時 `out_buf = null`、`out_buf_len = 0`。如果抽出的檔案長度是 0，`out_buf_len < needed` 不成立，程式會走到 `copy_nonoverlapping(extracted.as_ptr(), null, 0)`。`std::ptr::copy_nonoverlapping` 的文件規定，即使 count 是 0，指標也必須非 null 而且對齊，所以這是 UB。Rust ≥ 1.78 的 debug build 會檢查這個前置條件，失敗時走 `panic_nounwind`，也就是 abort。同一個檔案裡的其他兩段式函式都有提早 return，不會走到這裡（`:346-348`、`:508-510`），`c_abi/paver.rs:237` 也有防護。
- **可重現的情境：** PAMT 裡有一個 `compressed_size = uncompressed_size = 0` 的檔案。用 `out_buf = NULL, out_buf_len = 0` 呼叫 `crimson_paz_extract_file`。預期：release 是 UB（實務上多半沒事）；debug / 測試 build 會 abort。
- **未能驗證：** 真實 install 有沒有 0 位元組的條目。
- **嚴重度：** low。
- **確信度：** plausible。
- **來源：** 既有問題（`6b75402`）。

### A-08 — PAMT 邊界檢查的 u32 加法溢位

- **位置：** `src/binary/pamt.rs:351`。
- **問題與根因：** `raw_file.chunk_offset + raw_file.compressed_size` 兩邊都是 u32。debug build 會溢位 panic（C ABI 回 `PANIC`）；release build 會 wrap，例如 `0xFFFF_FF00 + 0x200 = 0x100`，於是通過 `<= chunk.size`。之後 `extract_file` 會 seek 到很大的 offset，`read_exact` 失敗，最後回 `BODY_PARSE`，所以 release 實際上只是讓驗證失效。
- **可重現的情境：** 條目 `chunk_offset = 0xFFFF_FF00`、`compressed_size = 0x200`、chunk `size = 0x1000`。預期 debug：`pamt.rs:351` panic；release：`PackMeta::parse` 成功。
- **嚴重度：** low。
- **確信度：** confirmed（程式碼）／plausible（後果）。
- **來源：** 既有問題（`8623be0`）。
- **建議方向：** `u64::from(a) + u64::from(b) > u64::from(chunk.size)`，或 `checked_add`。

### A-09 — PAPGT 舊版佈局的負長度

- **位置：** `src/binary/papgt.rs:170-174`；`src/binary/mod.rs:85-94`（`check_remaining` 用 `offset + need`）。
- **問題與根因：** 名稱緩衝區長度用 `i32` 讀，再 `as usize`。負值會被符號延伸成接近 `usize::MAX` 的數。`check_remaining` 的 `offset + need` 在 debug build 會溢位 panic；在 release 會 wrap 成 `offset - k`，通過檢查，然後 `data[offset..offset+buf_len]` 因為 end < start 而 panic。走到這裡要先通過 Jenkins checksum（可以重算）。2.04+ 的 signed 佈局用 `u32` 判斷（`:103-104`），負值不會成立，所以會落到舊版分支。這是唯一一處 `check_remaining` 的 `need` 可能超過 u32 的地方。
- **可重現的情境：** 舊版 PAPGT：`entry_count = 0`，名稱長度寫 `FF FF FF FF`，重算 checksum。呼叫 Python `parse_papgt_bytes`。預期 panic，Python 端看到的是 `PanicException`（它是 `BaseException`，`except Exception` 接不到）。C ABI 不解析 PAPGT。
- **嚴重度：** low。
- **確信度：** plausible。
- **來源：** 既有問題（`8623be0`）。

### A-10 — Roundtrip 不對稱

- **位置與根因：**
  1. **`COptional`**：`src/binary/types.rs:283`（讀：`flag != 0` 就是 `Some`）對上 `:296`（寫：永遠 `1`）。flag 是 `0x02` 的資料寫回後會變成 `0x01`。
  2. **`CFlaggedArray`**：`types.rs:224-228`（讀：flag ≠ 0 就再讀一個 `CArray`，`01 00000000` 讀成空列表）對上 `:234-235`（寫：空列表一律寫成一個 `00`）。「存在但為空」寫回後少 4 個位元組，也改變了後面所有欄位的位置。這是計畫書 F 區塊審計重點 2「缺席／存在但空／存在且非空」三態問題在 gamedata 層的同一種形狀。
  3. **Paver 未知尾段**：`src/binary/paver.rs:190-194` 對不認得的長檔退回 10 位元組讀法（文件說是「容忍 PA 加欄位」），但 `:216-230` 只寫回 10 位元組，多出來的部分會被丟掉。`parse_extra_bytes_tolerated`（`:268-277`）只測讀取，沒測寫回。
  4. **PAPGT 舊版尾端**：`papgt.rs:164-181` 讀完名稱緩衝區後，不檢查是否剛好到檔尾。原檔的 checksum 涵蓋多出來的位元組，但 `to_bytes`（`:265-283`）丟掉它們並重算 checksum。
- **可重現的情境：** 例如把某個 iteminfo 列裡 `CFlaggedArray` 的 `00` 改成 `01 00 00 00 00`，parse 再 write：輸出比輸入少 4 個位元組。
- **緩解因素：** 2.06 iteminfo 在 7,068 + 8,218 列上 byte-perfect（CLAUDE.md），所以目前的真實資料裡不存在情況 1 和 2，這兩種是潛在問題，等下一次佈局變動才可能出現。PAPGT 舊版只到 2.03。
- **嚴重度：** low。
- **確信度：** confirmed（程式碼不對稱）／plausible（實際資料會出現）。
- **來源：** 既有問題（`8623be0`；`CFlaggedArray` 是 `1f32ec2`；paver 是 `61c2f52`）。
- **建議方向：** 讀到不認得的 flag 值就回錯誤，或把原始 flag 存下來；`CFlaggedArray` 記住「flag=1、count=0」；paver 保留未知尾段。

### A-11 — `CArray` 上限的單位不對

- **位置：** `src/binary/types.rs:144-154`（`BinaryReadTracked` 版本在 `:192-203`）。
- **問題與根因：** 檢查的是 `count <= remaining_bytes`，可是 `Vec::with_capacity(count)` 實際配置的是 `count * size_of::<T>()`。對於 `size_of` 是幾十到幾百位元組的結構，6 MB 的 iteminfo 裡一個損壞的 count，就可以要求數百 MB 到數 GB 的配置。註解（`:139-143`）說的「避免多 GB 配置」只對 `T = u8` 成立。
- **與 C 區塊的關係：** 計畫書 C 區塊有「`TailSizeCache` 多 GB 配置」這條待查事項。skill 解析器沒有用 `CArray`（它用自己的 `with_capacity`，見 `skill_info/buff_data.rs:129,141`、`skill_info/mod.rs:373,382`），所以**本條不是那個問題的根因**。這裡只指出 iteminfo 等使用 `CArray` 的解析器有同一類風險，交給 C 區塊判斷。
- **可重現的情境：** 在 iteminfo 列中，把一個 `CArray<大型結構>` 的 count 改成接近剩餘位元組數。預期解析時先配置 `count × size_of::<T>()`，然後才在讀元素時失敗。
- **嚴重度：** low。
- **確信度：** plausible。
- **來源：** 既有問題（`b038c2d`）。
- **建議方向：** `with_capacity(count.min(remaining / max(1, 最小 wire 大小)))`，或乾脆不預先配置，讓 `push` 自然成長。

### A-12 — DDS per-mip 解碼器的算術與迴圈上限

- **位置：** `src/binary/paz.rs:253-306`（`mip_count`、寬、高直接取自 DDS 表頭的 u32）、`:308-327`（`width * height * (bpp/8)`、`bw * bh * 16`）、`:329-339`（迴圈 `mip_count` 次）、`:151-157`（`collect` 成長度 `mip_count` 的 `Vec<usize>`）、`:173`、`:189`、`:198`（`x + y` 型的邊界檢查）。
- **問題與根因：** 這些乘法和加法在 debug 會溢位 panic，在 release 會 wrap。`mip_count` 最大到 2³²，`expected_total_size` 就會跑 2³² 次迴圈（數秒的 CPU）。如果再搭配宣告約 4 GiB 的 `uncompressed_size`，讓 `expected_total_size == uncompressed_size` 成立（例如 1×1 DXT1、約 5 億層），`raw_mip_sizes` 會配置約 4 GB。這些都要先解密出一個構造的 DDS，而且策略 2（`:124`）會先依宣告值配置（A-02），所以實際影響被 A-02 蓋過。
- **嚴重度：** low。
- **確信度：** plausible。
- **來源：** 既有問題（partial 解碼器，`8623be0` 之後的 PAZ partial 支援）。

### A-13 — 版本閘門沒有實作成函式

- **位置：** `src/binary/paver.rs:62`、`:77`、`:107`、`:121`（四個常數）、`:115-116`（文件裡的公式）；`src/c_abi/paver.rs:151-243`（只匯出常數與 allow-list）；`docs/c-abi-surface.md:350-351`。
- **問題與根因：** CLAUDE.md 的不變式要求 `MAJOR/MINOR` 成對讀，再加上 `PARSER_MIN_GAMEDATA_PATCH`。crimson-rs 把完整的閘門公式（`major == MAJOR && minor ∈ COMPATIBLE && (minor != MINOR || patch >= MIN)`）寫在兩處文件裡，但 Rust 和 C ABI 都**沒有**一個實作它的函式，也沒有單元測試餵 `(2,5,9)`、`(2,6,0)`、`(1,6,0)` 之類的組合去釘住公式。每個消費端都得自己重寫一次，而公式裡最容易寫錯的正是「patch 下限只對 target minor 生效」。另外，crimson-rs 自己的解析器也從不讀 paver，閘門完全交給呼叫端。
- **計畫書審計重點 5 的結論：** 常數本身是成對、而且彼此一致的（`COMPATIBLE_GAMEDATA_MINORS = [6]`、floor = 0、`compatible_minors_includes_target` 有測），文件也正確。缺的是可執行、可測試的單一實作。
- **可重現的情境：** C# 端實作成 `minor ∈ compatible && patch >= minPatch`（漏了 `minor != target` 的條件）。在目前的值（只有一個 minor、floor 0）下行為相同，看不出錯。等到 allow-list 再次放進多個 minor、floor 不是 0 時（2.04 / 2.05 都發生過），就會誤擋或誤放。
- **嚴重度：** low。
- **確信度：** confirmed（沒有這個函式，也沒有這組測試）。
- **來源：** 既有問題（`61c2f52`；`PARSER_MIN_GAMEDATA_PATCH` 是 2.04.08 時加的；常數值在本次 `95fa25f` 更新）。
- **建議方向：** 加一個 `pub fn is_parser_compatible(&Paver) -> bool`，以及 `crimson_parser_is_compatible(major, minor, patch) -> u8`，並寫表格式測試。

### A-14 — A 區塊的關鍵測試在 CI 上不跑，或者根本不存在

- **位置：** `.github/workflows/ci.yml:57-58`（`cargo test --lib`，沒有 `--features c_abi`）；`src/c_abi/checksum.rs:87-115`（唯一的 Jenkins KAT，只在 `c_abi` 下編譯）；`src/crypto/checksum.rs:97-119`（只測「空輸入」、「確定性」、「不同輸入不同輸出」）；`src/crypto/chacha20.rs`（沒有測試）；`src/binary/paz.rs:757-1260`（沒有宣告值不符的解壓縮測試；builder 測試全部是 `CryptoType::None`）。
- **問題與根因：**
  1. CI 不開 `c_abi`，所以 `c_abi/{checksum,paloc,paver,paz}.rs` 的測試，包括不需要 live install 的合成測試（`c_abi_paloc_smoke`、`c_abi_paloc_loads_203_container`、paver 的 bytes 測試、checksum KAT），在 CI 上全都不會執行。
  2. 預設 feature 下，Jenkins 雜湊只有「空輸入 = 0xDEBA1DCD」一個已知答案。改壞 mix 或 finalize 的常數，CI 仍然是綠的。
  3. ChaCha20 完全沒有 KAT。依賴升級（例如 `cipher` 的 seek 語意改變）不會被 CI 抓到，只能靠在 CI 上會 skip 的 live 抽檔測試。
  4. A-03 / A-04 / A-06 的情境都沒有測試。
- **嚴重度：** low（屬於 I 區塊審計重點 1 的一個具體例子）。
- **確信度：** confirmed。
- **來源：** 既有問題。
- **建議方向：** 把 3 組 KAT 搬到 `crypto/checksum.rs`；加一組用合成檔名產生的 ChaCha20 KAT（可以用目前實作產生後固定下來）；CI 加一步 `cargo test --lib --features c_abi`（合成測試不需要 install）。

### A-15 — Python-only builder 的靜默截斷

- **位置：**
  - `src/binary/papgt.rs:239`：`entry_count = entries.len() as u8`，第 256 筆會 wrap 成 0，寫出的檔案 count 和實際條目數不一致。`:222`：`group_name_offset += insert_len as u32` 沒有檢查溢位。`:212-216`：名稱內含 NUL 時會把緩衝區切壞。
  - `src/binary/trie.rs:333`：`frag_bytes.len() as u8`，超過 255 位元組的片段只寫入長度的低 8 位元，但內容全部寫入，整個 trie 會錯位。
  - `src/binary/paz.rs:487`、`:495-496`、`:523`（chunk 大小與檔案大小 `as u32`）、`:493`（`chunk_id as u16`）、`:666`（`count as u16`）。單一檔案或單一 chunk ≥ 4 GiB 會被截斷。另外，單一檔案比 `max_chunk_size` 大時，還是會整個放進同一個 chunk（`:481-485`）。
  - `src/binary/paz.rs:529`、`:560`：`std::fs::write` 會直接覆寫 `output_dir` 裡已有的 `N.paz` / `0.pamt`（和 H 區塊審計重點 4 重疊）。
- **嚴重度：** low（只有 `python` feature 用得到；在合理的 mod 大小下不會觸發）。
- **確信度：** confirmed（程式碼）／plausible（後果）。
- **來源：** 既有問題（`8623be0`、`6d9184a`）。
- **建議方向：** 用 `u8::try_from` / `u32::try_from` 回 `InvalidInput`（`build_pamt` 處理副檔名數量時已經這樣做，`:609-614`）。

### A-16 — PALOC handle 的重複 key 與記憶體洩漏

- **位置：** `src/c_abi/paloc.rs:43-49`；`src/binary/types.rs:33-40`（以及 `:86-90`）。
- **問題與根因：**
  1. `by_key` 用 `collect` 建立，重複的 key 由**最後一筆**勝出；`entries` 則保留全部。所以 `crimson_paloc_lookup` / `lookup_str`（被其他 bridge 拿來做名稱解析）和 `crimson_paloc_get_entry` 的列舉，對同一個 key 可能給出不同的值，而且 `entry_count` 包含重複的筆數。真實 PALOC 有沒有重複 key 沒有驗證過。
  2. `CString::read_from` 遇到非 UTF-8 時會 `Box::leak` 一份 lossy 字串，讓它活得跟 `'a` 一樣久。在長時間執行的 C# 宿主裡，每次重新載入 PALOC（例如切換語言）都會再洩漏一次，永遠不會回收。iteminfo 等其他 `CString` 使用者也一樣。
- **嚴重度：** low。
- **確信度：** plausible（真實資料有沒有重複 key 或非 UTF-8 字串都未驗證）。
- **來源：** 既有問題。

### A-17 — Paver 的退回讀法與 `BinaryRead` panic

- **位置：** `src/binary/paver.rs:190-194`、`:207-213`。
- **問題與根因：** 2.04+ 的 stamp 如果 DLC 尾段讀不懂（例如 PA 在 revision 後面加了欄位，或 DLC 名稱不是 UTF-8），會退回 10 位元組讀法，`build` 就變成 DLC 數量（例如 1），而 C ABI 照樣回 OK（`c_abi/paver.rs:44-45` 也只說短於 10 位元組才回 `BODY_PARSE`）。`major/minor/patch` 不受影響，所以閘門不會錯，只有 `build` 錯。`BinaryRead::read_from` 的 `&data[*offset..]` 在 offset 超過長度時會 panic（目前沒有呼叫端這樣用）。
- **嚴重度：** nit。
- **確信度：** plausible。
- **來源：** 既有問題（`61c2f52`；DLC 尾段來自 2.04）。

### A-18 — 文件與程式碼脫節

全部是 confirmed、nit、既有問題：

1. `src/c_abi/paz.rs:130-133`：文件說 `.paz` chunk 讀不到時回 `IO`，但 `extract_file` 的所有 `io::Error`（包括 `.paz` 不存在、`read_exact` EOF）在 `:212` 都變成 `BODY_PARSE`。只有 PAMT 本身讀不到才是 `IO`。
2. `src/c_abi/paz.rs:17-23` 與 `:131-133`：說 worldmap SDF 和 partial 檔案「still surface as `BODY_PARSE`」，但 `binary/paz.rs:62-71,108-111` 已經實作 per-mip 策略，專門處理 worldmap SDF；`binary/paz.rs:965` 的 live 測試也把 worldmap 當成策略 3 的成功案例。
3. `docs/c-abi-surface.md`：六個 `crimson_paloc_*` 符號（`load_from_file`、`load_from_bytes`、`free`、`entry_count`、`lookup`、`get_entry`）沒有逐一列出，只在模組圖 `:443-444` 有一行。這不符合計畫書「列出的函式與實際 `extern "C"` 符號一一對應」的檢查。同一張模組圖 `:406` 把 `binary/paz.rs` 描述成「PackGroupBuilder, compression, PAZ creation」，漏了抽取與 partial 解碼。
4. `src/c_abi/paloc.rs:238-241`：key 不是合法 UTF-8 時回 `NULL_ARG`，但「Returns」清單（`:200-206`）沒有列出這種情況。`:154-158`：`crimson_paloc_free` 的文件只提 `load_from_file`，沒有提 `load_from_bytes`。
5. `src/binary/pamt.rs:447-448`：註解說「the actual post-header checksum is validated by the papgt entry, not stored in the pamt header itself」，但 `:212-213` 明明用 PAMT 表頭自己的 checksum 驗證。
6. `src/crypto/checksum.rs:1`、`src/c_abi/checksum.rs:1,28`、`docs/c-abi-surface.md:417`：都叫它「hashlittle2」，但實作只回傳 `c`，也就是 lookup3 的 `hashlittle`；`hashlittle2` 會同時回傳 `(c, b)`。演算法本身是對的（見排除說明），只是名稱不對。
7. `src/c_abi/paver.rs:12`：模組文件說 `read_from_bytes`「parses a 10-byte buffer」，2.04+ 已經是變長。

### A-19 — Feature gate／lint 小問題

全部是 confirmed、nit、既有問題：

- `src/binary/papgt.rs:3`：`#[allow(unused_imports)]` 標在每種 feature 組合下都有用到的 `read_cstring`（`:179`）上，這個 allow 已經過時。
- `src/binary/pamt.rs:12-19`：`Compression::Partial` 從來沒有被建構（partial 用 `is_partial` 表示），靠 `allow(dead_code)` 遮住。
- `src/c_abi/paloc.rs:30`：含 `HashMap` / `Vec` 的不透明 handle 標了 `#[repr(C)]`，沒有意義（B-11 有同樣的模式）。
- Feature gate 本身沒有問題：A 區塊在 `default` 下只依賴無條件的 `lz4_flex` / `flate2` / `chacha20` / `hmac` / `sha2`；只給 Python 用的 builder 靠 `src/lib.rs:12-15` 的 crate 級 `cfg_attr` 消除警告，而 CI 的全 feature clippy（`ci.yml:51`）仍然能抓到在所有組合下都沒用到的程式碼。這是 `lib.rs` 註解寫明的設計，我沒有發現被它遮住的真正 dead code。

---

## 未能驗證

| 項目 | 需要什麼 | 影響哪些條 |
|---|---|---|
| live install 的 PAMT 有沒有 zlib（comp 3）條目、未知 nibble、u32 加法溢位、0 位元組條目 | 唯讀掃描全部 `0.pamt` 的 20 位元組檔案條目（不需要解密） | A-03、A-05、A-07、A-08 |
| 有沒有加密檔的「檔名雜湊 + ⌈compressed_size/64⌉」超過 2³²−1 | 同上，加上 trie 名稱解碼與 Jenkins 雜湊（與 `c_abi/checksum.rs` 的 KAT 交叉驗證） | A-06 |
| `lz4_flex 0.11.6` 短輸出是否回 Ok；`chacha20 0.10` / `cipher 0.5.1` 計數器用盡與 seek 越界是否 panic | 讀 `~/.cargo/registry` 的原始碼，或寫一個最小重現 | A-02、A-03、A-06 |
| 真實 PALOC 是否有重複 key 或非 UTF-8 字串 | 抽出各語言 PALOC 後統計 | A-16 |

以上都需要執行程式（或存取本 session 允許範圍外的目錄），超出「僅靜態審查」的限制，所以留給下一輪。

---

## 排除說明

計畫書「掃描時注意到的待查事項（未驗證）」裡**沒有**標給 A 區塊的項目。下面是 A 區塊審計重點與跨區塊關注點中，經查證**沒有問題**的部分：

1. **Jenkins checksum 的邊界（審計重點 4）**：`crypto/checksum.rs:40-79` 與 Bob Jenkins lookup3 `hashlittle` 逐步一致。`mix` 的位移 4/6/8/16/19/4、`final` 的 14/11/25/16/4/14/24 都正確；初值 `0xDEBA1DCD + len` 等於 `initval = 0xDEBA1DCD − 0xDEADBEEF`；`while remaining > 12` 讓最後一個（可能剛好 12 位元組的）區塊進入 final；長度 0 回 `c`，和 lookup3 的 `case 0: return c` 一致；1–11 位元組的尾段以 0 補齊後相加，和 lookup3 逐位元組 switch 的結果相同。`c_abi/checksum.rs` 的三組 KAT 長度是 24（尾段 12）、43（尾段 7）和 29（尾段 5），涵蓋了完整尾段和部分尾段。沒有問題，只有命名（A-18 第 6 點）和 CI 涵蓋（A-14）兩個問題。
2. **ChaCha20 key/nonce 推導（審計重點 4）**：`chacha20.rs:21-41` 加密與解密共用同一函式，對稱；nonce 只取最後一個 `/` 之後的檔名，抽取端（`paz.rs:722-727`）與 builder（`paz.rs:464-476`）組出的路徑一致，包括 2.04+「stem + 副檔名表」重組後的檔名。只有 A-04（表頭 `encrypt_info` 不一致）和 A-06（計數器上限）兩個問題。
3. **PAPGT signature-like trailer 的 roundtrip（審計重點 3）**：signed 佈局由「各段長度剛好加總到檔尾」判定（`papgt.rs:95-105`），trailer 原樣存在 `Option<[u8; 64]>`（`:152-153`），寫回時原樣附在最後（`:255-263`），沒有重算或修改。`signed_layout_roundtrips` 與 live 的 `live_install_pamt_and_papgt_roundtrip`（`paz.rs:1204-1237`）都有涵蓋。`add_entry` 的文件明說 trailer 不會更新、遊戲是否接受未確立（`:199-200`），這符合 CLAUDE.md「不要繞過 trailer」的規則。本次審計沒有嘗試分析或重算 trailer。
4. **Paver 三種形狀的 roundtrip（審計重點 3）**：10 位元組（`dlcs: None`）、14 位元組（DLC 數量 0）、DLC 清單三種都會原樣寫回（`paver.rs:216-230`；測試 `round_trip_bytes`、`parse_stamp_without_dlc`、`parse_2_04_dlc_stamp`）。10 位元組的 stamp 不可能被誤讀成 DLC 尾段（讀完 count 後沒有空間放 `build`）。`read_dlc_tail` 的 count 有跟檔案長度比對，沒有預先配置（`:153-156`），名稱切片用 `checked_add`（`:159`）。只有「未知尾段」（A-10 第 3 點）和退回讀法（A-17）兩個問題。
5. **`paver` 版本判定成對（審計重點 5）**：常數成對、彼此一致，文件公式正確，live 測試同時比對 major 與 minor（`c_abi/paver.rs:367-383`）。缺的只有可執行的單一實作（A-13）。
6. **PALOC 2.03 容器**：`unwrap_container`（`paloc.rs:53-99`）檢查保留欄位、區塊長度剛好到檔尾、0x11–0x1FF 全為 0、`body_len <= block_len × 255 + 16`（擋掉不合理的配置），解壓縮後也比對長度。`LocalizationFile::parse` 用「每筆最少 16 位元組」擋住尾端 count，並要求剛好讀到 count 前面（`:177-202`）。沒有問題。
7. **2.04+ `RelativeParent` trie 的循環**：距離必須 ≥ 1 且 ≤ offset，ULEB128 最多 5 個位元組（`trie.rs:63-91`），所以不會形成循環。剩下的只有深度問題，已經寫進 A-01。
8. **PAMT 的其他邊界**：目錄的檔案範圍用 usize 加法，並跟 `raw_files.len()` 比對（`pamt.rs:315-328`）；副檔名索引用 `get`（`:332`）；未知 chunk id 回錯誤（`:343-348`）；2.04+ 檔尾不能有多餘位元組（`:273-278`）；`header.count` 是 u16，最多配置 786 KB。沒有問題。
9. **C ABI 的 null 與兩段式呼叫**：`crimson_paz_*`、`crimson_paloc_*`、`crimson_paver_*`、`crimson_calculate_checksum` 都會先檢查 null 和「buf 為 null 但 len ≠ 0」，再進入 `catch_unwind`。緩衝區不足時都會先填好 required / count 再回 `BUFFER_TOO_SMALL`。`CrimsonPazFileEntry` 有 `const` assert 檢查大小 272、對齊 4（`c_abi/paz.rs:401-402`），也有 offset 測試（`:1084-1101`）。唯一的例外是 A-07。
10. **`Cargo.toml` 的 panic 策略**：沒有 `[profile] panic = "abort"`，用的是預設的 unwind，所以 C ABI 的 `catch_unwind` 對一般 panic 有效。A-01、A-02 的「abort」指的是 stack overflow 和配置失敗這兩種本來就不經過 unwind 的終止。
