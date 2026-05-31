<p align="center">
  <img src="docs/logo.png" width="120" alt="Inari logo">
</p>

<h1 align="center">Inari</h1>

<p align="center">
  <a href="README.md">English</a> | Tiếng Việt
</p>

Trình quản lý môi trường dev di động cho Windows, kiểu chép-là-chạy. Đóng gói
sẵn nginx, PHP, MariaDB và Redis sau một bảng điều khiển nhỏ gọn, giúp bạn chạy
trọn bộ stack PHP mà không cần cài đặt gì.

Inari là một phần của bộ công cụ **SushiBox**.

> Giống Laragon hay XAMPP, nhưng di động ngay từ đầu: giải nén thư mục, bấm đúp,
> là stack chạy. Không đụng tới registry hay `Program Files`.

> **Beta.** Inari còn ở giai đoạn sớm. Nó chạy tốt bộ stack dev muh5 và đã test
> trên Windows 11. Có thể còn lỗi vặt, rất mong nhận báo lỗi.

<p align="center">
  <img src="docs/panel.png" width="380" alt="Bảng điều khiển Inari">
</p>

## Tính năng

- **Một cửa sổ, mọi dịch vụ.** Chạy, dừng, khởi động lại nginx, PHP-CGI,
  MariaDB và Redis từ một bảng điều khiển gọn.
- **Di động thật sự.** Tất cả nằm trong một thư mục. Chép sang USB hay máy khác
  là chạy được. Không trình cài đặt, không cần quyền admin.
- **Cấu hình ưu tiên GUI.** Cổng, thư mục web, tự khởi động, chế độ tối và ngôn
  ngữ đều chỉnh trong bảng điều khiển. Không phải sửa file cấu hình bằng tay.
- **Tự khởi động khi mở.** Chọn dịch vụ nào sẽ chạy khi bạn mở Inari.
- **Khay hệ thống (tray).** Đóng cửa sổ là thu xuống tray; stack vẫn chạy.
- **Giao diện tiếng Anh và tiếng Việt**, đổi trong Cài đặt.

## Bắt đầu nhanh

1. Tải gói portable và giải nén ra thư mục bất kỳ.
2. Bấm đúp `Inari.exe`. Bảng điều khiển mở ra (không có cửa sổ console).
3. Bấm **Chạy tất cả**, hoặc chạy từng dịch vụ.
4. Trang web được phục vụ từ `sites/default` tại <http://localhost:8080>.

Dùng nút **Mở trang web** trong bảng điều khiển để mở site trong trình duyệt.

> **Lưu ý SmartScreen.** Inari chưa được ký số, nên Windows SmartScreen có thể
> hiện "Windows protected your PC" ở lần chạy đầu. Bấm **More info → Run
> anyway**. Gói là mã nguồn mở; bạn có thể tự build (xem phần dưới).

### Cổng mặc định

| Dịch vụ | Cổng |
|---|---|
| Bảng điều khiển | 1788 |
| nginx (web) | 8080 |
| MariaDB | 3307 |
| Redis | 6380 |

PHP-CGI lắng nghe nội bộ ở cổng 9000. Mọi dịch vụ chỉ bind vào `127.0.0.1`.

## Code của bạn

Đặt dự án PHP vào `sites/default` (hoặc trỏ **Thư mục web** trong Cài đặt sang
thư mục khác). nginx định tuyến `*.php` sang PHP-CGI; file tĩnh phục vụ trực
tiếp. Gói có sẵn `index.php` mẫu và endpoint JSON `health.php` để bạn kiểm tra
stack hoạt động.

## Dòng lệnh

Để tự động hóa hoặc viết script, `inari-cli.exe` cung cấp các lệnh tương tự kèm
output trên console:

```
inari-cli.exe start      # chạy tất cả dịch vụ có sẵn
inari-cli.exe stop       # dừng các dịch vụ đang chạy
inari-cli.exe restart    # khởi động lại tất cả
inari-cli.exe status     # xem trạng thái dịch vụ và cổng
```

`Inari.exe` là bản GUI (không console); `inari-cli.exe` là bản CLI không giao diện.

## Cấu hình

Hầu hết thiết lập nằm trong bảng điều khiển. Để chỉnh mặc định, sửa
`flavors/default.lua` (cổng, sites, hooks). Bảng điều khiển ghi đè vào
`data/settings.json`, file này thắng so với flavor. nginx.conf và php.ini được
sinh tự động mỗi lần khởi động, tinh chỉnh sẵn cho phát triển local.

## Yêu cầu

- Windows 10/11, hoặc Windows Server 2019 trở lên (64-bit).
- Trên Windows 11 và Server 2025 dùng WebView2 (Edge) hệ thống. Trên Server
  2019/2022, kèm bản WebView2 fixed-version trong gói (xem `runtime/manifest.toml`).

## Build từ source

Inari là một Rust workspace cộng với panel viết bằng Nuxt.

```
# Build panel (được nhúng vào binary)
cd panel && bun run build

# Build GUI và CLI
cargo build --release

# Đóng gói bản portable vào dist/
powershell -ExecutionPolicy Bypass -File scripts/package-portable.ps1
```

Các binary runtime (nginx, PHP, MariaDB, Redis, Adminer) không được commit. Tải
chúng bằng `scripts/fetch-runtime.ps1` trước khi đóng gói.

## Ảnh chụp

Cài đặt — tab Chung và Dịch vụ (cổng, tự khởi động, thư mục web):

<p align="center">
  <img src="docs/settings-general.png" width="300" alt="Cài đặt — Chung">
  &nbsp;
  <img src="docs/settings-services.png" width="300" alt="Cài đặt — Dịch vụ">
</p>

Dính góc dưới-phải, trên thanh taskbar, kiểu PowerToys / PC Manager:

<p align="center">
  <img src="docs/desktop.jpg" width="600" alt="Inari dính góc dưới-phải">
</p>

Giao diện tối và tiếng Anh:

<p align="center">
  <img src="docs/panel-dark.png" width="300" alt="Giao diện tối">
  &nbsp;
  <img src="docs/settings-en.png" width="300" alt="Giao diện tiếng Anh">
</p>

## Giấy phép

Phần code của Inari dùng giấy phép MIT (xem [LICENSE](LICENSE)).

Inari đóng gói phần mềm runtime của bên thứ ba, mỗi thứ theo giấy phép riêng.
Xem [THIRD_PARTY.md](THIRD_PARTY.md) để biết danh sách đầy đủ và link nguồn. Lưu
ý: MariaDB server đi kèm dùng GPL-2.0; nếu bạn phân phối lại gói Inari, điều
khoản cung cấp source của GPL sẽ áp dụng.

## Liên kết

- Trang chủ: [greenjade.net](https://greenjade.net/)
