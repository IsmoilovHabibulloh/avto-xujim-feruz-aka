# VIP Ads v1

Rust + Grammers asosidagi Telegram Ads tekshiruvchi userbot va bitta serverdan ishlaydigan MUI admin panel.

## Nima bor

- Rust `axum` backend.
- React + MUI admin panel.
- Admin login: `Feruz` / `Feruzboy`.
- Telegram userbot ulash: API ID, API hash, telefon, kod, 2FA parol.
- `contacts.getSponsoredPeers` orqali har bir key (kalit so'z) bo'yicha GLOBAL sponsored qidiruv.
- `messages.viewSponsoredMessage`, `messages.clickSponsoredMessage`, `messages.reportSponsoredMessage` chaqirilmaydi.
- Interval va keylar (kalit so'zlar) paneldan sozlanadi. Alohida "tekshiriladigan kanallar" ro'yxati yo'q — qidiruv global.
- Default interval: 5 sekund.
- Natijalar `data/state.json`da, Telegram session `data/userbot.session`da saqlanadi.

## Muhim izoh

Har bir key Telegram serveriga `contacts.getSponsoredPeers` orqali global qidiruv query sifatida yuboriladi. Server o'sha query bo'yicha sponsored kanallarni qaytaradi; topilgan kanal oq ro'yxatda BO'LMASA SMM order yuboriladi (oq ro'yxatda bo'lsa — yo'q). Alohida qora ro'yxat yo'q.

Bir xil reklama qayta topilsa, oldingi order holati tekshiriladi: bajarilgan bo'lsa qayta yuboriladi, bajarilmagan bo'lsa kutiladi; 10 daqiqada ham bajarilmasa baribir qayta yuboriladi.

Telegramning ichki MTProto holatlari va server tomondagi barcha hisob-kitoblarini yashirish kafolatlanmaydi. Bot faqat sponsored qidiruv natijasini oladi va ko'rildi/click/report requestlarini yubormaydi.

## Lokal ishga tushirish

```bash
cp .env.example .env
npm install --prefix frontend
npm run build --prefix frontend
cargo run -p vipads-server
```

Keyin oching:

```text
http://127.0.0.1:8080
```

## `.env`

```bash
HOST=0.0.0.0
PORT=8080
PUBLIC_DOMAIN=avto-feruz-aka.vipads.uz
ADMIN_USERNAME=Feruz
ADMIN_PASSWORD=Feruzboy
STATE_PATH=data/state.json
TELEGRAM_SESSION_PATH=data/userbot.session
STATIC_DIR=frontend/dist
```

## Serverga deploy

1. Serverga kodni joylang.
2. Node va Rust o'rnating.
3. Frontend build qiling:

```bash
npm install --prefix frontend
npm run build --prefix frontend
```

4. Backendni release build qiling:

```bash
cargo build --release -p vipads-server
```

5. Systemd service namunasi:

```ini
[Unit]
Description=VIP Ads server
After=network.target

[Service]
WorkingDirectory=/opt/vipads
EnvironmentFile=/opt/vipads/.env
ExecStart=/opt/vipads/target/release/vipads-server
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
```

6. Nginx reverse proxy:

```nginx
server {
    server_name avto-feruz-aka.vipads.uz;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

SSL uchun odatda:

```bash
certbot --nginx -d avto-feruz-aka.vipads.uz
```

## Admin panel oqimi

1. `Feruz` / `Feruzboy` bilan kiring.
2. `Userbot` tabida API ID, API hash va telefon kiriting.
3. `Kod olish` bosing.
4. Telegramdan kelgan kodni kiriting.
5. Agar 2FA yoqilgan bo'lsa, 2FA parolni kiriting.
6. `Sozlamalar` tabida keylar (kalit so'zlar), interval va oq ro'yxatni sozlang. Sozlamalar avtomatik saqlanadi.
7. `Natijalar` tabida avtomatik yoki qo'lda scan natijalarini ko'ring.

## Manbalar

- Grammers: https://codeberg.org/Lonami/grammers
- Telegram TL schema ichidagi ads metodlari: `messages.getSponsoredMessages`, `messages.viewSponsoredMessage`
