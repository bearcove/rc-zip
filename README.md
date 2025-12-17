[![MIT OR Apache-2.0 licensed](https://img.shields.io/badge/license-MIT+Apache_2.0-blue.svg)](./LICENSE)
[![Crates.io](https://img.shields.io/crates/v/rc-zip)](https://crates.io/crates/rc-zip)
[![docs.rs](https://docs.rs/rc-zip/badge.svg)](https://docs.rs/rc-zip)

# rc-zip

![The rc-zip logo: a person with long hair and nice brows being opened from the bottom up with a zipper. On dark backgrounds, you can see a skull being revealed. On light backgrounds you cannot.](https://github.com/user-attachments/assets/5fecd286-9518-4a72-b544-56675d7f31f6)

_Logo by [MisiasArt](https://misiasart.com)_

### Motivasyon

Saf Rust ile yazılmış, yüksek uyumluluğa sahip, G/Ç (I/O) modelinden bağımsız bir zip okuma ve yazma kütüphanesi olmak.

(Not: Şu an itibariyle rc-zip yalnızca okuma yapabilmektedir.)

### Finansman

rc-zip üzerindeki sözleşmeli çalışmaları için bu şirketlere teşekkürler:

<a href="[https://rowzero.io](https://rowzero.io)"><img src="./static/rowzero-e.svg" height="40"></a>

Ve tüm [bireysel sponsorlarıma](https://fasterthanli.me/donate) teşekkürler.

### Tasarım Kararları

Bu kütüphanenin (crate) çekirdeği doğrudan G/Ç işlemi gerçekleştirmez. Bunun yerine bir durum makinesi (state machine) kullanır ve belirli ofsetlerde okuma yapılmasını talep eder. Bu, kütüphanenin farklı G/Ç modelleri altında çalışmasına olanak tanır: bloklayan (blocking), bloklamayan (non-blocking) ve asenkron (async). Zip arşivinin diskte bulunması gibi bir beklentisi yoktur (yani `std::fs` varlığını varsaymaz); sadece rastgele erişimin (random access) mümkün olması yeterlidir.

Önerilen arayüz, yerel başlıklar (local headers) yerine merkezi dizine (central directory) dayanır:
```
[local file header 1] // <---------------- ignored
[file data 1]
[local file header 2]
[file data 2]
[central directory header 1] // <--------- used
[central directory header 2]
[end of central directory record]
```

Bunun nedeni, merkezi dizinin (central directory) bir zip dosyasındaki girişlerin asıl (canonical) listesi olmasıdır. Yeniden paketlenen arşivler, silinmiş girişlerin başlıklarının yanı sıra mükerrer yerel dosya başlıkları (ve verileri) içerebilir. Bir zip arşivinin içeriği söz konusu olduğunda yalnızca merkezi dizin yetkilidir.

Bununla birlikte, v4.0.0 itibarıyla, `ReadZipStreaming` trait'leri şeklinde hem `rc-zip-sync` hem de `rc-zip-tokio` kütüphanelerine akışlı (streaming) bir sıkıştırma açma arayüzü eklenmiştir.

Bu kütüphane, "trailing zips" (sondaki zipler) olarak bilinen dosyaları kabul eder; örneğin geçerli ELF veya PE yürütülebilir dosyaları olan ve sonuna geçerli bir zip arşivi eklenmiş dosyalar. Bu, bazı kendi kendine açılan arşivleri ve yükleyicileri kapsar.

Bu kütüphane zip64 meta verilerini tanır ve kullanır. Bu, çok sayıda girişe (65536'nın üzerinde) ve büyük girişe (4GiB'ın üzerinde) izin verir. Bu kütüphane, yaygın araçlardan kaynaklanan bazı standart dışı davranışları tolere etmeye çalışır. Bu tür davranışlar gerçek dünyada gözlemlenmiş ve mümkün olduğunca test edilmiştir.

Bu kütüphane mümkün olduğunca fazla meta veriyi tanımaya ve normalleştirmeye çalışır. Örneğin; MSDOS zaman damgaları, NTFS zaman damgaları, Genişletilmiş zaman damgaları ve Unix zaman damgaları desteklenir ve bunların tamamı bir [chrono DateTime<Utc>](https://crates.io/crates/chrono) nesnesine dönüştürülür.

Meta verilerin normalleştirilmiş versiyonu (isimler, zaman damgaları, UID, GID vb.) ön planda tutulsa da, bu kütüphane yazarların gerekli hissettiği her an aynı meta verilerin "ham" (raw) versiyonunu da sunmaya çalışır.

Zip arşivi açıkça UTF-8 kodlaması belirtmediğinde, bu kütüphane CP-437 ve Shift-JIS arasında karar vermek için kodlama algılamasına (encoding detection) güvenir. Shift-JIS ile başa çıkmak için [encoding_rs](https://crates.io/crates/encoding_rs) kullanır.

Zip formatının geçmişi nedeniyle bazı uyumluluk sorunları beklenebilir: örneğin, yalnızca MSDOS zaman damgalarına sahip arşivler için sonuçlar yanlış saat diliminde olabilir. Çok az dosyası ve UTF-8 olmayan isimleri olan arşivlerde, kodlama düzgün algılanamayabilir ve bu nedenle kod çözme işlemi başarısız olabilir.

Zip arşivi formatında kullanılan çeşitli veri yapılarını ayrıştırmak için mümkün olduğunca [winnow](https://crates.io/crates/winnow) kullanılır. Bu, yazılması, okunması ve gerektiğinde değiştirilmesi daha kolay olan yarı bildirimsel (semi-declarative) bir stil sağlar. Tekrardan kaçınmak için bazı (hijyenik) makrolar kullanılmıştır.

### API Tasarımı

API tasarımı birkaç parametreyle kısıtlanmıştır:

* Uyumlu bir zip okuyucusu, önce zip arşivinin sonuna yakın bir yerde bulunan merkezi dizini **okumalıdır**. Bu, sadece bir `Read` almanın yeterli olmayacağı anlamına gelir.
* Birden fazla G/Ç (I/O) modeli desteklenmelidir. Diğer kütüphaneler bir `Read`, bir `Read + Seek` veya sadece bir bayt dilimi almaya odaklanırken; bu kütüphane senkron **ve** asenkron G/Ç'yi desteklemeyi amaçlar.
* Herkes uyumlu bir zip okuyucusu istemez. Bazıları merkezi dizini tamamen görmezden gelerek sadece yerel başlıklara güvenmek ve risk almak isteyebilir.

Sonuç olarak, bu kütüphanedeki struct'lar; okuma yapma (ve nereden), veri işleme veya yazma ihtiyaçlarını bildiren durum makineleridir (state machines). G/Ç hataları diğerlerinden temiz bir şekilde ayrılmıştır ve bu kütüphaneye yapılan çağrılar asla bloklamaz (block).

rc-zip üzerine belirli G/Ç modelleri ekleyen ayrı kütüphaneler için [rc-zip-sync](https://crates.io/crates/rc-zip-sync) ve [rc-zip-tokio](https://crates.io/crates/rc-zip-tokio) kütüphanelerine bakınız.

## Lisans

Bu proje öncelikli olarak hem MIT lisansı hem de Apache Lisansı (Sürüm 2.0) koşulları altında dağıtılmaktadır.

Detaylar için [LICENSE-APACHE](https://www.google.com/search?q=LICENSE-APACHE) ve [LICENSE-MIT](https://www.google.com/search?q=LICENSE-MIT) dosyalarına bakınız.
