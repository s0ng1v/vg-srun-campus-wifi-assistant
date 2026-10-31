# VG Campus Wi-Fi Assistant

VG 校园网助手是一个面向 Windows 10/11 的深澜 Web Portal 自动认证工具。
它在电脑启动后保持一个轻量的本地守护进程，负责检测连接、自动登录和断线恢复。

当前状态：开发中。认证流程目前只在长安大学门户完成真实环境验证。

## 当前范围

- 深澜 Web Portal 认证
- 自动发现门户，失败时粘贴登录页地址
- Wi-Fi 与有线网络
- 多账号顺序故障转移
- Windows 登录前自动运行
- GUI 完全退出后继续守护
- 本地加密保存凭据，无遥测

暂不支持 802.1X、PPPoE、验证码、短信验证和多学校门户并存。

## 开发环境

- Windows 10/11
- Node.js 20+
- Rust 1.90+
- WebView2

```powershell
npm install
npm run dev
```

## 安全说明

- 本项目不会绕过学校计费、设备数量或访问策略。
- 账号认证仅允许通过 HTTPS 提交。
- 仓库不包含真实账号、密码、日志或浏览器会话。
- 使用本项目产生的网络行为仍受学校网络管理规定约束。

## 许可证

MIT。第三方组件许可见 `THIRD_PARTY_NOTICES.md` 和 `licenses/`。

