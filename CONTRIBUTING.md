# Contributing

## Development checks

```powershell
npm ci
npm audit --audit-level=high
npm run check:lens
npm run build:renderer
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

## Protocol changes

- Use the bundled mock portal tests and fixed Python reference vectors.
- Never add real credentials, packet captures or production logs to fixtures.
- Treat authentication error codes as portal-specific compatibility data.
- Do not weaken HTTPS requirements or local IPC access controls.

## Commits

Use concise Chinese Conventional Commits, for example:

```text
feat(daemon): 加入断线状态机与指数退避
fix(ipc): 修正守护快照的并发读取
docs(security): 补充诊断日志脱敏规则
```

