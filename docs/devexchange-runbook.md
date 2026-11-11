# DevDay Exchange — FULL EVENT-DAY SCENARIO (2026-11-07, Da Nang)

> The complete runbook: door to door. Demo shape + Q&A pocket live at the bottom.
> Companion file to docs/devexchange-demo.md's demo/Q&A sections (merged here, single source).

## BEFORE LEAVING HOME (morning)

- [ ] Bag: laptop + charger, phone (100%, mobile data active), phone charger, venue address
- [ ] Machine: `docker pull` the v0.1.3 image ALREADY DONE (no venue-bandwidth gambling)
- [ ] Offline copies in a folder: this file, recorded fallback clips, key screenshots
- [ ] Tabs pre-bookmarked: Solscan (pool program), GitHub Actions red run (m9/doctrine-suite), repo
- [ ] Leave home with the stack ALREADY TESTED this morning (healthz 200 from cold boot)
- [ ] PRE-BUILD IS OFFICIAL: organizers encourage building at home ("make some progress
      with us") — so prep the M9 spike WORKSPACE before leaving: m9 branch checked out,
      Codex→pool config tested, the afternoon must look like progress, not setup.

## OFFICIAL SCHEDULE FACTS (organizer info, 2026-11-04)

- Credits ($100 Codex + $50 API) arrive 24–48h AFTER the event — NOT at check-in.
  → The demo runs on YOUR OWN home-funded provider key. The door-credits line is DEAD.
- 14:00–14:30 check-in (QR scan gates the post-event credits) · 14:30–15:00 opening + showcase
  rules · 15:00–18:00 build session (tea breaks) · **18:00–19:30 ALL demos (≈90 min total →
  limited slots → SIGN UP AT CHECK-IN)** · 19:30–19:50 all-attendee voting + prizes · 19:50–20:00
  close/photos.
- Voting is everyone, right after demos — votes are actually WON during the build-session
  conversations, not on the stage.
- **DEMO MECHANIC (organizer-confirmed):** NO pre-registration for demos — after the build
  session, hosts call for volunteers; hands go up in the room; ~10–15 slots of 3–5 minutes.
  → TACTIC: when hosts ask, your hand goes up FIRST and confidently (early slot = remembered;
  3–5 min means the COMPRESSION CUT is your primary shape, not the fallback).
- **LANGUAGE (corrected ruling):** deliver in VIETNAMESE — the voters are local builders; VN
  gives you comfort, timing, and humor under stage pressure. Technical terms stay EN naturally.
  Three lines stay EN (for the international hosts + quoting): the thesis (beat 6), the close
  (beat 7), and the RWA shield line in Q&A. VN script below the EN one.
- No slides required or expected — live screen demo is the norm (matches our no-slides call).

## BEFORE-LEAVING ADDITION

- [ ] Open Luma while logged in → read the EXACT venue address (visible once "You're In") →
      plan the route; screenshots of the QR ticket on the phone AND a second copy in email.

## 13:45 — ARRIVE

1. Check in at the door — QR scan FIRST (it gates the post-event credits).
2. Ask organizers THREE logistics questions (do this FIRST, they get busy):
   - How do I sign up for a demo slot — how many slots are there? (90-min window ⇒ limited.)
   - Wi-Fi name + password.
   - Where are the power outlets I can camp at?
3. Pick a seat: outlet power + visible enough that people wander past your screen.

## 14:00–15:00 — SETUP (check-in window; finish during opening talks)

1. Boot; `docker compose up` the seeded v0.1.3 stack → `curl /healthz` → 200.
2. Phone: join tailnet → open the ts.net URL on MOBILE DATA (the D32 leg's road test).
3. Codex is pointed at the pool with YOUR OWN home-funded provider key (Gemini free-tier
   verified, or your OpenAI key) — event credits arrive only after, irrelevant to today.
4. Run ONE test completion through Codex → verify a usage row appears in the dashboard.
   THE METER IS LIVE — everything from now on is demo material.
5. Open and pin: Solscan txs, red-run Actions tab, dashboard, terminal.

## 15:00–18:00 — BUILD SESSION

- Work the M9 spike with Codex (3pm–6pm = the "since 3pm" story writes itself).
- Tea breaks are the real campaign: 2–3 real conversations, ask about THEIR builds first —
  network as a builder, not a pitcher. These voters decide during the build, not at 19:30.
- Glance at the dashboard now and then — you're watching the demo material accumulate.

## 17:30–18:00 — PRE-DEMO LOCK-IN

- [ ] PRE-STAGE: pre-create demo org + admin (BEAT 2 nêu lý do: "tạo sẵn để tiết kiệm thời gian") — member + key tạo LIVE trên sân khấu
- [ ] One whisper-rehearsal of the 6 beats at your seat (phone leg timing included)
- [ ] Screen hygiene: close personal tabs; order = terminal → dashboard → Solscan → red-run
- [ ] Phone to 100%; mobile data ON; Solscan tabs refreshed
- [ ] Confirm your slot time + length with organizers

## THE DEMO (scripted for 5 min; compress/expand guide below)

| Beat | Màn hình | Câu nói |
|---|---|---|
| 1. Mở 50s | terminal | "Ai từng share API key cho team hay CLB… qua group chat? Share key AI giống sinh viên ở trọ — ở chung thì rẻ, nhưng group chat là ở trọ không hóa đơn, không khóa phòng. Tributary là nhà trọ đúng nghĩa: mỗi người key riêng, hạn mức riêng, hóa đơn rõ. Và builder nào định tự viết — đừng viết, cứ lấy đi." |
| 2. Admin→Member→Luật · 2.5ph | dashboard → terminal → điện thoại | Pre-tạo org+admin (nêu lý do) → LIVE: login admin, thêm member, tạo key → curl completion → burst 429 → usage rows vừa tạo → audit log → D32 flag từ ĐIỆN THOẠI (4G) → review queue + một dòng classifier |
| 3. Vault & người thuê 100s | Solana txs + Solscan | "V2 là chợ credit: người dư góp vào pool, người cần — renter — nạp trước, dùng qua key riêng, KHÔNG cần ví. Deposit → claim như vé gửi đồ; prepaid → escrow không ai di chuyển được; refund đúng phần chưa dùng; depositor kiếm từ đúng phần bị đốt. *[Solscan]* receipt công khai vĩnh viễn." |
| 4. Vì sao chain / RWA 70s | Solscan/repo | "Sổ sách quyết định ai được nhận tiền — chain mới khiến họ thực sự nhận được. RWA đưa tài sản lên chain để trade; Tributary đưa nghĩa vụ lên chain để settle. Claim như vé gửi đồ — không trade được. Oracle của mình dễ hơn vàng: usage dashboard của provider là nguồn chân lý bên thứ ba." |
| 5. Luận điểm 15s | — | *(EN)* "My product governs how agents spend tokens; my process governs how agents build the product. Verifiable rules instead of trust." |
| 6. Kết 15s | repo/profile | *(EN)* "Open source — the link is on my profile. Star it, or bring me your team's key problem. Thank you!" |

- **Nếu chỉ có 3 phút:** Mở → Beat 2 (rút: bỏ audit + classifier) → Vault → Kết.
- **⏱ TIME BUDGET — CẦN BÁM ĐỒNG HỒ:** full script nói hết ≈ 7 phút; slot thật là 3–5 phút.
  Tập với timer, và tự cắt theo thứ tự ưu tiên: (1) Beat 2 — audit log rút còn MỘT câu, bỏ
  dòng classifier nếu gấp; (2) Beat 4 — bỏ ví dụ vàng, giữ hai câu lõi; (3) Beat 1 — bỏ câu
  "đừng tự viết" nếu gấp. KHÔNG cắt: demo 429 + tx refund + câu thesis.
- **Nếu được 8+ phút:** thêm CHUYỆN M9 sau beat 4 — "milestone tuần này bắt đầu bằng 13 test fail, mỗi test tên là một rule; một agent session mới đang implement theo; red run public trong repo." *(red CI run làm đạo cụ)*
- **Không biết câu trả lời:** *(EN)* "Great question — the repo's open, let's look together." Không bao giờ đoán bừa.

## FULL SPOKEN SCRIPT — TIẾNG VIỆT (tập từ đây; [ngoặc] = động tác)

**[BEAT 1 — MỞ · 50s]** [bước lên, terminal trên màn hình, cười]

> "Chào mọi người, em là Phat. sv nam hai khoa cntt truong đại học bách khoa đại học đà nãng. Thôi, 
> giói thiệu vậy thôi câu h quá btc đám em chet :))). Rồi e có 1 câu hỏi cho mn đo là h nếu mn muốn tiết kiệm chi phí dung AI cho 1 team, tổ chúc nho nhỏ tầm 5-10 ng thì cái chiến luoc đầu tiên mn nghĩ đến là gì? ... 
> Ok để e trả lòi luôn, thì cái chiến luoc của e cũng đon giản nhu mọi thàng sv khác mà thôi, "dùng chung", bọn sv trc h 
> hay đi dùng chung trọ, vói cả ba cái tài nguyên học tập đồ cũng dùng chung cho tiết kiệm, thì h có thêm 1 cái để dùng chung nũa đó là AI credit.
> Và dùng chung ỏ đây cụ thể là mn sẽ build 1 cái pool, xong rồi deposit api key vào đó rồi issue key cho các member có thể burn usage của các key trong pool, ... Này e thấy nhiều team dev h cũng làm rồi, và nó cũng chảng còn ms lạ gì, tuy nhiên vấn đề ỏ đây đó là việc build các cái pool nhu thế này khá tốn thòi gian và cũng ko dễ gì đối vói cả các anh chị dev đã đi làm, và các anh chị thì cũng ko cs nhiều time và động lục để build cho nó chỉnh chu ấy, kiểu cũng luoi :)), nên là thôi, bản thân e là 1 thàng sv còn đang rảnh nên e xin mạn phép gánh bót mấy cái này cho anh chị :))) tù h nếu muốn build mấy cái pool kiểu này thì cú lên github của e - phat1911 tìm repo tên tributary xong rồi làm theo huong dẫn cái folder deploy, trong đó em có viết script và ghi detail cx khá kỹ.
**[BEAT 2 — ADMIN → MEMBER → MỌI LUẬT, TRÊN SÂN KHẤU · 2.5ph]** [dashboard → terminal → điện thoại]

> "Giờ em sẽ cho nó chạy thật. Mình đã tạo sẵn org và admin *trước khi lên sân khấu* — để tiết kiệm
> thời gian; còn lại làm live hết.
> *[dashboard: login admin]* Đây là admin đăng nhập. *[thêm member]* Mình thêm một member —
> *[tạo key]* — và tạo key cho member này: `sk-pool-…`. Từ giây này, nó là một người dùng
> thật.
> *[terminal: curl]* Mình gửi request bằng curl — *nói rõ: curl chỉ là cách nhanh nhất trên
> sân khấu. Key này cắm vào Codex, vào bất kỳ tool nào tương thích OpenAI là chạy — chiều nay
> Codex của mình đã uống từ pool này cả buổi. Giờ dùng curl cho gọn thời gian.*
> Request đi qua gateway: check quota TRƯỚC, rồi mới tới provider — và mọi thứ được meter.
> Giờ xem quota — mình set member này vài request mỗi phút. *[burst]* …một, hai, ba — và:
> **429. Limit reached.** Và refusal này xảy ra TRƯỚC khi gọi provider — không ai lãng phí
> tiền.
> *[dashboard: usage]* Đây — mấy request mình vừa gửi đã nằm trong usage: bao nhiêu, model
> gì, key nào. Còn audit log: ai, làm gì, khi nào, từ IP nào. Ghi một lần, không bao giờ sửa.
> Và leak detection: *[giơ điện thoại]* điện thoại mình đang 4G — một mạng khác. Cùng một
> member key… *[gửi một request từ điện thoại]* …và đây: *cùng key, hai mạng, cách nhau vài
> phút.* Nó vào review queue — và lưu ý triết lý: auto-block *khóa* key, không bao giờ *cấm*
> người. Response luôn là con người.
> *(Một dòng nữa: pool còn bộ phân loại mục đích sử dụng — key cấp để code mà đi hỏi chuyện
> khác sẽ bị gắn cờ; ai cũng bật được cho key của mình — xem trong repo.)*"

**[BEAT 3 — VAULT & NGƯỜI THUÊ CREDIT · 100s]** [Solana explorer + các tab Solscan]

> "Tầng cuối cùng — tiền. V2 là chợ credit AI: người có credit *dư* góp vào pool; người
> *cần* credit — renter — đến thuê. Renter làm gì? Chọn pool, nạp trước số tiền mình muốn,
> rồi dùng qua key riêng của mình — giống mọi member. Renter KHÔNG cần ví, không chạm chain
> — chain là plumbing phía sau, người dùng chỉ thấy AI chạy. Với gateway, renter chỉ là một
> member key — mọi luật y như nhau.
> Xem cơ chế, live trên devnet. Deposit: depositor góp credit — on-chain chỉ mint một *claim
> record*, không có tiền di chuyển — vault vẫn zero. Claim giống vé gửi đồ: không bán được,
> transfer fail ngay ở token layer.
> Tiền vào khi renter prepaid — vào escrow do *program* kiểm soát. Không một cá nhân nào di
> chuyển được — kể cả mình. Không dùng hết? Refund bằng chính key của renter — *[tx
> refund]* — vault trả đúng phần chưa dùng. Và depositor kiếm tiền từ đúng phần credit của
> họ bị đốt — mọi dòng tiền đều đi qua vault này.
> *[chỉ vào Solscan]* Đây là Solscan — trình duyệt công khai của Solana, giống Etherscan với
> Ethereum: ai cũng mở được, không cần tài khoản, không cần tin ai. Và mọi money moment của
> pool mình là một trang như thế này — receipt công khai, vĩnh viễn. Ai trong phòng mở link
> này đều xem được."

**[BEAT 4 — VÌ SAO BLOCKCHAIN / RWA · 70s]** [giữ màn hình Solscan/repo]

> "Nhiều người sẽ hỏi: sao cần blockchain ở đây? Ngắn gọn: sổ sách quyết định *ai được nhận
> tiền* — nhưng chain mới là thứ khiến họ *thực sự nhận được tiền*. Nếu tất cả nằm off-chain,
> operator vừa giữ tiền vừa viết sổ — như một relay với file Excel. Với chain thì khác: escrow
> không một cá nhân nào di chuyển được, exit mở từ phía người dùng, và nói dối phải trả giá
> bằng bond.
> Và nếu ai nghĩ theo hướng RWA — tokenize vàng, tokenize trắm khố — phần khó của họ cũng là
> phần khó của mình: *chain không nhìn thấy tài sản off-chain.* Cùng machinery, nhưng mục
> đích ngược nhau: RWA đưa *tài sản* lên chain để trade; Tributary đưa *nghĩa vụ* lên chain
> để settle. Claim của mình như vé gửi đồ — không bán được, không trade được, transfer fail
> ngay ở token layer. Và oracle của mình còn dễ hơn vàng: usage dashboard của OpenAI, Google
> là nguồn chân lý bên thứ ba — depositor nào cũng tự kiểm tra được. Kho vàng không bao giờ
> có thứ đó."

**[BEAT 5 — LUẬN ĐIỂM · 15s]** [bước lùi khỏi laptop] *(nói bằng EN)*

> "One sentence: my product governs how agents *spend* tokens. My process governs how agents
> *build* the product. Verifiable rules instead of trust — that's the whole idea."

**[BEAT 6 — KẾT · 15s]** *(nói bằng EN)*

> "It's open source — the link is on my profile and in the event chat. Star it, or bring me
> your team's key problem. Thank you!"

*Lưu ý tập: đọc mỗi beat hai lần, rồi đóng file lại và nói bằng lời của mình. Các động tác
trong [ngoặc] là choreography — không bao giờ đổi; lời nói có thể bẻ.*

## Q&A POCKET (memorize line 2 of each; the rest is backup)

**RWA / tokenization:** "The hard part of RWA is the chain can't see the off-chain asset — same
problem here, my credits are provider accounts. You can't solve it at mint time; you cage it:
fake claims pay nothing, every mint is a public event naming its signer, books and chain must
reconcile, and settlement is a Merkle tree anyone can challenge — proven fraud slashes a bond.
And our oracle is easier than gold's: providers' usage dashboards are third-party truth."
KEY LINE: "RWA brings assets on-chain to trade them; Tributary brings obligations on-chain to
settle them."

**Why blockchain:** "The books decide who's owed; the chain is what makes them pay. Without it
the operator holds the money AND writes the ledger — a relay with a spreadsheet."

**vs relay hubs (new-api etc.):** "Relays sell access with opaque balances. We sell provable
settlement: escrow, ceilings, challenge windows. Obligations, not tokens." (Never name them.)

**Is the claim a security/token:** "It's a coat-check ticket — transfers refuse at the token
layer. No sale, no market, no trading."

**Who pays gas / wallets:** "The operator's backend sponsors fees; users never see a wallet.
The chain is back-end plumbing."

## AFTER THE DEMO

- Watch 2–3 other showcases fully; VOTE visibly (be seen voting for others).
- Connection requests same evening (3–5, one personal line each) — hosts included if you spoke.
- Photo of the room/your screen → next week's post ("what DevExchange taught me").
- WRITE DOWN every question people asked — especially ones you fumbled = GDG rehearsal notes.

## LEAVING

- Export the day's usage_events (the "a live harness drank from my pool at DevExchange" story).
- Tear down anything throwaway you created (workflow step 13 — even at a venue :))
- Thank the organizers in Vietnamese on the way out.

## CONTINGENCY LADDER (per beat: live → recorded → screenshot)

- **NO DOOR CREDITS (or arrive late):** the dogfood NEVER depended on them — it needs *a*
  provider key, and yours is configured AT HOME (Gemini free-tier verified, or your OpenAI key).
  Default posture: run on YOUR key all afternoon. IF door credits land, point a second provider
  at them and keep the "event's electricity" line — it is a BONUS line, spoken only if true.
  Without it, beat 3's close becomes: "…my agent's been drinking from the pool all afternoon."
- **NO DEMO SLOT (too many showcases / time runs out):** volunteer FIRST at check-in (the
  logistics questions), and if still cut — the CORRIDOR DEMO: same script, 1-on-1 at your
  table during breaks. Twenty two-minute table demos to curious builders beat six minutes on
  a stage. And the build session itself already was the demo — they watched it all afternoon.
- Venue Wi-Fi dead → everything except Solscan/red-run tabs is local; tabs were pre-opened.
- Devnet stalls → Solscan tabs already show today's txs; narrate from receipts.
- Codex-through-pool config fails at venue → usage rows from the MORNING home test; same story,
  "this morning's session" instead of "since 2pm."
- Laptop dies → phone: Solscan + red-run + repo (the whole thesis survives in 3 tabs).
