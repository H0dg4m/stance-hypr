# stance

**Hyprland için seni siyah ekranda bırakmayan ekran ayarları.** Her
değişiklik onaylanmazsa kendiliğinden geri döner, bar çökse bile. Yenileme
hızını, çözünürlüğü, ölçeği, konumu ya da yerleşimi bardan iki tıkla değiştir,
ekranları haritada sürükle, kendi **modların** (oyun, çalışma, tek ekran…)
arasında geç: bardan, tak-çalıştır kuralıyla, dizüstü kapağıyla, şarj
takılınca ya da telefonundan.

[English](README.md)

<p align="center">
  <img src="docs/panel.png" alt="stance paneli" width="330">
  &nbsp;
  <img src="docs/confirm.png" alt="onayla ya da geri dön" width="330">
</p>

## Neden

Ekran araçlarının çoğu "uygulamayı aç, ayarla, kapat" mantığında ya da yalnız
arka planda profil değiştirir. stance bar için yapıldı:

- **Bardan iki tık**: canlı yerleşim haritası olan küçük bir panel.
- **Onayla ya da geri dön**: her ekranda 12 sn geri sayım; hiçbir şey yapmazsan
  eski düzen geri gelir. Bar çökse bile çalışır (zamanlayıcıyı ayrık bir bekçi
  süreç tutar).
- **Ekranın ötesinde modlar**: bir mod çalışma alanlarını ekranlara bağlayabilir,
  animasyon / bulanıklık / tearing açıp kapatabilir, boşlukları ve güç
  profilini değiştirebilir. Yalnız beyaz liste: mod dosyası komut çalıştıramaz.
- **Hızlı ayarlar**: gece ışığı (hyprsunset), parlaklık (dizüstü paneli ve
  DDC/CI ile harici ekran) ve ses çıkışı aynı panelde ve modlarda.
- **Tak-çalıştır kuralları** (isteğe bağlı daemon): dock'u tak, masa modun gelsin.
- **Boşta sıfır maliyet**: CLI tık başına çalışıp çıkar (~1 ms, ~1,5 MB);
  isteğe bağlı daemon Hyprland olay soketinde bekler (~3 MB bellek).

## Kurulum

Arch: `stance-hypr` / `stance-hypr-git` (`packaging/aur` içinde PKGBUILD; AUR yüklemesi bekliyor). Kaynaktan:

```sh
cargo build --release
install -Dm755 target/release/stance ~/.local/bin/stance
stance doctor
```

Hyprland **0.55+** ve Lua config gerekir (0.57 ile test edildi; 0.57 hyprlang
`.conf` desteğini kaldırdı). 0.55/0.56 için `.conf` desteği deneyseldir.

## Bara ekleme

- **Quickshell** (end-4 / illogical-impulse ve türevleri ya da kendi kabuğun):
  hazır bileşenler, bkz. [docs/quickshell.md](docs/quickshell.md).
- **Başka bar** (Waybar, …): paketteki bağımsız kabuğu bir kez başlat, tıklamaya
  `qs -p /usr/share/stance/quickshell ipc call stance toggle` bağla.
- **Bar yok**: her şey CLI'dan çalışır.

## CLI

```
stance monitors                       ekranları listele
stance set DP-1 --hz 165              bir ekranı değiştir
stance set eDP-1 --off                (son açık ekranı kapatmayı reddeder)
stance layout right-of eDP-1 DP-1     left-of | right-of | above | below | mirror | single
stance arrange DP-1=0x0 eDP-1=2560x360  birden çok ekranı birlikte taşı
stance confirm | stance revert        bekleyen değişikliği koru / geri al
stance persist                        ~/.config/hypr/stance-monitors.lua yaz
stance mode save work                 mevcut düzeni mod olarak kaydet
                                      (--with-quick: gece ışığı + ses çıkışı da)
stance mode apply gaming
stance night on 4500 | off | toggle   hyprsunset ile gece ışığı
stance brightness DP-1 +10            N | +N | -N yüzde (DDC/CI ya da backlight)
stance audio set "Razer"              varsayılan ses çıkışı: id, düğüm adı ya da tekil metin
stance quick                          gece ışığı, parlaklık ve ses tek çağrıda
stance daemon                         tak-çalıştır kuralları (systemd birimi contrib/ içinde)
stance doctor                         yalnız okuyan ortam denetimi
```

`<monitor>` açıklama (`"JDR EPIC Q40"`), `"marka model"` ya da bağlantı adı
olabilir. Değişiklik 12 sn içinde onaylanmazsa geri döner (`--timeout S`,
betikler için `--no-confirm`). Tüm komutlar `--json` ile JSON konuşur.

Mod ve kural dosyalarının biçimi için İngilizce README'deki örneklere ve
[`examples/`](examples) klasörüne bak. Modlarda `night_light = false | true | K`,
`audio_sink = "…"` ve `[brightness]` tablosu (`"ekran" = yüzde`) da olabilir.
Bunlar ekran değişikliğinden sonra uygulanır, 12 sn geri alma kapsamında değildir
(ekranı karartamazlar); biri başarısız olursa (kulaklık takılı değil, DDC/CI yok)
mod yine uygulanır ve not düşülür. `--no-extras` bunları atlar.

### Modlar ve kurallar

Mod: `~/.config/stance/modes/<ad>.toml` (ekranlar, çalışma alanları, beyaz
listeli Hyprland ayarları, güç profili, gece ışığı, ses çıkışı, parlaklık).
Kurallar: `~/.config/stance/rules.toml`; bağlı ekran kümesi kuralınkine tam
eşitse mod uygulanır, hiçbiri eşleşmezse hiçbir şey olmaz. İkisi de panelden
düzenlenebilir (Ayarlar ve Modlar). Ayrıntılı örnekler İngilizce README'de ve
[`examples/`](examples) klasöründe.

### Hızlı ayarlar

- **Gece ışığı** hyprsunset soketiyle konuşur. Zaten çalışan bir hyprsunset varsa
  onu kullanır ve senin gamma ayarını korur; "kapalı" nötr 6000 K'dir, sıfırlama
  değil. Hiçbiri çalışmıyorsa `stance night on` bir tane başlatır.
- **Parlaklık**: dizüstü paneli `/sys/class/backlight` ve `brightnessctl` ile,
  harici ekran `ddcutil` ile (`i2c-dev` modülü ve `/dev/i2c-*` erişimi, genelde
  `i2c` grubu gerekir). Ekran tamamen kararmasın diye %5 (backlight) ve %1 (DDC)
  altı yükseltilir.
- **Ses çıkışı** `pw-dump` ile okunur, `wpctl set-default` ile değiştirilir.

Hangilerinin kullanılabildiğini `stance doctor` söyler.

### Telefon kumandası

Telefonu Stream Deck benzeri bir kumandaya çevirir: modlar, gece ışığı,
parlaklık, ses çıkışı ve ekran düzeni tuşları; ekran değişikliğinde
Koru / Geri al. Android uygulaması [`android/`](android) içinde (Kotlin,
Compose; QR okuma ZXing ile, Google hizmeti gerekmez; birden çok bilgisayar;
cihaz anahtarı Android Keystore ile sarılır). Kaynaktan derle ya da APK'yı
sürüm sayfasından kur.

<p align="center">
  <img src="docs/app-home.png" alt="eşli bilgisayarlar" width="200">
  <img src="docs/app-pair.png" alt="eşleştirme: parmak izini karşılaştır" width="200">
  <img src="docs/app-deck.png" alt="tuşlar ve Koru / Geri al şeridi" width="200">
</p>

```sh
stance deck serve            # ya da: systemctl --user enable --now stance-deck.service
stance deck pair             # uygulama için QR (tek telefon, 120 sn)
stance deck devices          # eşli telefonlar; stance deck unpair AD
```

1. Uygulamada **Add PC**'ye dokun, kodu tara (ya da yapıştır), parmak izini
   bilgisayardakiyle karşılaştır, **Pair**'e dokun. Eşleşince bilgisayar bir
   masaüstü bildirimi gösterir.
2. Bir tuşa bas. Ekran değişikliğinde telefonda **Keep / Revert** çıkar; kimse
   yanıt vermezse bilgisayardaki gibi kendiliğinden geri alınır.

Wi-Fi + QR ile eşleştirme ya da USB (`stance deck serve --loopback` ve
`adb reverse tcp:47800 tcp:47800`; kodun adresi `127.0.0.1` olmalı). Bağlantı
şifrelidir (Noise), yalnız eşli telefonlar girer, tuşlar yalnız
`~/.config/stance/deck.toml` içindeki eylemleri çalıştırır; "komut çalıştır"
tuşu yoktur. Güvenlik duvarı kullanıyorsan yerel ağdan TCP 47800'e izin ver
(`stance doctor` söyler).

Tuşları panelde (Ayarlar → Phone buttons) ya da `stance deck buttons`,
`stance deck put --data JSON`, `stance deck reset` ile düzenle.

<p align="center">
  <img src="docs/phone-buttons.png" alt="Panel ayarlarında telefon tuşları" width="330">
</p>

Ayrıntı ve protokol: [docs/deck.md](docs/deck.md),
[docs/protocol.md](docs/protocol.md); uygulama derleme ve testler:
[android/README.md](android/README.md).

### Ekranlar yerini hatırlar

stance her ekranın son konumunu, modunu ve ölçeğini saklar
(`~/.local/state/stance/memory.json`). Kapatıp açılan ekran eski yerine döner;
daemon çalışıyorsa ve hiçbir kural eşleşmiyorsa çıkarıp yeniden takılan ekran da.
Panelde ekranlar haritada sürüklenerek taşınır (kenarlar yapışır), modlar
panelden eklenip düzenlenir, ayarlar sayfasından gösterilecek bölümler seçilir.

### Dizüstü kapağı ve güç

```lua
-- hyprland.lua: kapak kapanınca dahili ekran kapanır (başka ekran açıksa),
-- açılınca eski yerine döner. Onay sayacı yok: kapak kapalıyken görülmez.
hl.bind("switch:on:Lid Switch", hl.dsp.exec_cmd("stance lid close"), { locked = true })
hl.bind("switch:off:Lid Switch", hl.dsp.exec_cmd("stance lid open"), { locked = true })
```

Tak-çalıştır kuralları güç kaynağına bağlanabilir (`on_power = "ac" | "battery"`);
daemon açıksa şarj takılıp çıkarılınca kurallar yeniden değerlendirilir
(`stance power` mevcut kaynağı ve geçerli kuralı gösterir). Waybar için
`stance waybar` ve [contrib/waybar](contrib/waybar).

### Kalıcı yapmak

Çalışma anı değişiklikleri bilerek geçicidir (projektör ayarı kalıcı olmasın).
`stance persist` config klasörüne `stance-monitors.lua` yazar; ana config'ine bir
kez `require("stance-monitors")` ekle (nwg-displays kullanıyorsan onun
`require("monitors")` satırından sonra). stance ana config'ine asla dokunmaz.

## Güvenlik

- Hyprland soketine doğrudan bağlanır; `hyprctl` ve kabuk hiç kullanılmaz.
- Dış araçlar (`ddcutil`, `brightnessctl`, `wpctl`, `pw-dump`, `powerprofilesctl`,
  `hyprsunset`) mutlak yol, argüman listesi, kesin zaman aşımı ve boş stdin ile
  çalışır; çıktıları ayrıştırılır, asla çalıştırılmaz. Parlaklık hedefi ve ses
  çıkışı önce canlı cihaz listesinde çözülür; mod dosyası yalnız gerçek cihazlar
  arasından seçebilir.
- Lua yalnız tipli ve doğrulanmış değerlerden üretilir. EDID metinleri
  compositor'a hiç gitmez, yalnız gösterilir (kontrol karakterleri temizlenir).
- Mod/kural dosyaları: en çok 64 KiB, bilinmeyen alan reddedilir, sahibi sen
  olmalısın ve grup/herkes yazamamalı, komut alanı yoktur.
- Yazmalar atomiktir (`O_EXCL|O_NOFOLLOW` geçici dosya, fsync, rename),
  symlink hedefleri reddedilir. Durum 0700 dizinde tutulur.
- Bekleyen değişiklik Hyprland oturumuna bağlıdır; eski oturumun anlık
  görüntüsü yeni oturuma uygulanmaz. Root gerekmez.

## Geliştirme

```sh
cargo test                    # birim testleri (Noise test vektörleri dahil)
tests/nested.sh lua           # atılabilir iç içe Hyprland'de uçtan uca
cd android && ./gradlew :core:test -Dstance.binary=$PWD/../target/debug/stance
                              # telefon tarafı: vektörler + deck serve ile canlı uyum
```

Yayın adımları: [RELEASE.md](RELEASE.md).

## Lisans

MIT
