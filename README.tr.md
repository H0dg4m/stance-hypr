# stance

Hyprland için bar öncelikli ekran ayarları ve **modlar**. Yenileme hızını,
çözünürlüğü, ölçeği, konumu ya da yerleşimi iki tıkla değiştir; kendi modların
(oyun, çalışma, tek ekran…) arasında geç. Siyah ekranda kalmazsın: her
değişiklik onaylanmazsa kendiliğinden geri döner.

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
- **Tak-çalıştır kuralları** (isteğe bağlı daemon): dock'u tak, masa modun gelsin.
- **Boşta sıfır maliyet**: CLI tık başına çalışıp çıkar (~1 ms, < 1 MB);
  isteğe bağlı daemon Hyprland olay soketinde bekler (~3 MB bellek).

## Kurulum

Arch (AUR): `stance-hypr-git` (`packaging/aur`). Kaynaktan:

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
stance confirm | stance revert        bekleyen değişikliği koru / geri al
stance persist                        ~/.config/hypr/stance-monitors.lua yaz
stance mode save work                 mevcut düzeni mod olarak kaydet
stance mode apply gaming
stance daemon                         tak-çalıştır kuralları (systemd birimi contrib/ içinde)
stance doctor                         yalnız okuyan ortam denetimi
```

`<monitor>` açıklama (`"JDR EPIC Q40"`), `"marka model"` ya da bağlantı adı
olabilir. Değişiklik 12 sn içinde onaylanmazsa geri döner (`--timeout S`,
betikler için `--no-confirm`). Tüm komutlar `--json` ile JSON konuşur.

Mod ve kural dosyalarının biçimi için İngilizce README'deki örneklere ve
[`examples/`](examples) klasörüne bak.

### Kalıcı yapmak

Çalışma anı değişiklikleri bilerek geçicidir (projektör ayarı kalıcı olmasın).
`stance persist` config klasörüne `stance-monitors.lua` yazar; ana config'ine bir
kez `require("stance-monitors")` ekle (nwg-displays kullanıyorsan onun
`require("monitors")` satırından sonra). stance ana config'ine asla dokunmaz.

## Güvenlik

- Hyprland soketine doğrudan bağlanır; `hyprctl` ve kabuk hiç kullanılmaz.
- Lua yalnız tipli ve doğrulanmış değerlerden üretilir. EDID metinleri
  compositor'a hiç gitmez, yalnız gösterilir (kontrol karakterleri temizlenir).
- Mod/kural dosyaları: en çok 64 KiB, bilinmeyen alan reddedilir, sahibi sen
  olmalısın ve grup/herkes yazamamalı, komut alanı yoktur.
- Yazmalar atomiktir (`O_EXCL|O_NOFOLLOW` geçici dosya, fsync, rename),
  symlink hedefleri reddedilir. Durum 0700 dizinde tutulur.
- Bekleyen değişiklik Hyprland oturumuna bağlıdır; eski oturumun anlık
  görüntüsü yeni oturuma uygulanmaz. Root gerekmez.

## Lisans

MIT
