# stance

Hyprland için bar öncelikli ekran ayarları ve **modlar**: çözünürlük, yenileme hızı, ölçek,
konum, VRR ya da yerleşimi iki tıkla değiştir; kendi modların (oyun, çalışma, tek ekran…)
arasında otomatik *onayla ya da geri dön* zamanlayıcısıyla geçiş yap.

[English](README.md)

> **Durum: erken geliştirme (M0).** Şimdilik yalnız okuma komutları çalışıyor.

## Kullanım

```
stance monitors [--json]   # ekranları listele
stance doctor [--json]     # ortamı denetle (yalnız okur)
```

Tüm komutlar `--json` destekler; bar arayüzleri bu çıktıyı kullanır.

## Tasarım hedefleri

- **Boşta sıfır maliyet:** CLI komut başına çalışıp çıkar; isteğe bağlı daemon (takılınca kurallar) olay bekler.
- **Varsayılan güvenli:** Hyprland soketine doğrudan bağlanır (`hyprctl` yok, kabuk yok), config'i yalnız
  tipli değerlerden üretir, mod dosyasından komut çalıştırmaz, root istemez.
- **Lua ve eski config:** Hyprland 0.55+ Lua config'i `eval` ile, hyprlang `.conf` geri dönüş yolu.

## Derleme

```
cargo build --release
./target/release/stance doctor
```

## Lisans

MIT
