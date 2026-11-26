# Remote server tự cài trên mọi host (thiết kế, chờ duyệt)

## Mục đích
Ai cài Spirit, kể cả build từ source, SSH vào host được hỗ trợ là tự có đủ tính năng, như VS Code Server.
Lỗi gốc: binary cần glibc 2.39, `bkai-3090` có 2.31; check trả `rc=1` nên không cài lại.

## Mục tiêu
- Server: Linux x86_64/aarch64 (glibc hoặc musl), macOS aarch64/x86_64. Client: macOS, Linux.
- Server không cần internet, root, Docker hay toolchain.
- Binary hỏng hoặc cũ tự được thay. Cài thất bại thì về SSH thường, kèm lý do.
- Giữ `ssh_extension_install_mode` (always ask / always / never).

## Ngoài phạm vi
Host Windows/BSD, kiến trúc khác, `$HOME` mount `noexec`, client Windows, build lúc SSH.

## Không được xảy ra
- Chạy binary tải về chưa khớp SHA-256.
- Dừng daemon đang phục vụ phiên khác để cập nhật.
- Lặp vô hạn check/cài.
- Ghi đè thay đổi chưa commit trong working tree.

## Phương án
- A. GitHub Releases, client tải và upload qua SSH: **đã chọn**. Dùng được cho bản build từ source.
- B. Nhúng vào app: offline nhưng nặng, bản source vẫn thiếu binary.
- C. Build khi SSH: chậm, cần toolchain.
Người dùng quyết định: release nằm ở repo GitHub riêng của họ.

## Thiết kế (đề xuất)
1. **Build**: workflow riêng trên GitHub-hosted runner. Linux dùng `script/linux/bundle --artifact cli --channel oss`
   (musl static); macOS build `warp-oss` bản CLI. Mỗi asset gồm binary và `bundled_resources`.
   Smoke test trước khi đăng: Ubuntu 18.04, 20.04, 24.04 và Alpine; ARM chạy trên runner ARM hoặc QEMU.
2. **Manifest** trong source: repo, release tag, và với mỗi OS/arch: tên asset, kích thước, SHA-256 của asset và binary.
   Release bất biến, không dùng `latest`. URL và hash đi cùng commit nên bản source luôn biết cần binary nào.
3. **Client**: override ở `~/.warp-oss/remote-binaries` -> cache theo hash -> tải HTTPS, verify, ghi atomic.
4. **Đường dẫn trên host**: `~/.warp-dev/remote-server/warp-oss-<release_id>` cho channel Oss.
   `release_id` chung cho mọi OS/arch vì `remote_server_binary()` không nhận platform. ID đổi thì là file mới, nên tự cập nhật và không đè binary đang chạy. `warp-oss` cũ bị bỏ qua.
   Dọn bản cũ không còn process dùng.
5. **Check**: exit 0 là dùng; 255, timeout hoặc signal là lỗi kết nối; mã khác là cần cài.
   Sau upload: verify hash, chạy `--version`, rồi đổi tên atomic. Mỗi phiên chỉ thử cài một lần.
6. **Preinstall**: bỏ điều kiện glibc/non-glibc khi artifact là static; vẫn ghi nhận libc để chẩn đoán.
   Socket/PID của daemon cũng gắn `release_id`, để client mới không nối vào daemon cũ.
7. **Tương thích**: server trả `release_id`, client so với manifest, thay cho `GIT_RELEASE_TAG` ở channel Oss (client có tag hiện từ chối server không tag).
   Override của dev dùng ID suy ra từ hash file local.
   Đổi giao thức phải tăng phiên bản giao thức và cập nhật manifest; CI chặn nếu thiếu.

## Nghiệm thu
- `bkai-3090` (2.31) và `bigdata` (2.39) tự cài không có thao tác tay; daemon và proxy kết nối được.
- Test: exit code, chọn asset, hash sai, cache, upload đứt, host offline, Linux ARM64, Alpine.
- Đổi release trong manifest thì lần SSH sau tự cập nhật.
- Clone source mới trên máy sạch, build client, SSH được.

## Rủi ro
Chưa build musl thử, nên việc đầu tiên là spike build x86_64 và chạy trên `bkai-3090`.
Chưa xác nhận runner GitHub miễn phí đủ RAM/thời gian cho `release-cli`.

## Để người thực hiện quyết định
Định dạng manifest, thông báo tiến độ, cách retry tải, chính sách dọn cache.
