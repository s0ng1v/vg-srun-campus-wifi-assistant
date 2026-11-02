import {
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { currentMonitor, getCurrentWindow } from "@tauri-apps/api/window";
import {
  Check,
  ChevronLeft,
  ChevronRight,
  Cpu,
  ExternalLink,
  Globe2,
  LockKeyhole,
  MemoryStick,
  Minus,
  ShieldCheck,
  Users,
} from "lucide-react";
import appIcon from "../src-tauri/icons/icon.png";
import {
  LensActionButton,
  LensChoiceAction,
  LensDialog,
  LensIconButton,
  LensMenu,
  LensNotificationHost,
  LensPanel,
  LensProgress,
  LensSearchField,
  LensStage,
  LensSwitch,
  LensTabs,
  LensToggleTile,
  animateTextChange,
  lensToast,
} from "@/components/lens-original";
import { useTheme } from "@/components/theme-provider";
import { DRAG_REGION_ATTR, DRAG_REGION_STYLE } from "@/lib/platform";
import { cn } from "@/lib/utils";
import {
  discoverPortal,
  getDaemonSnapshot,
  installSystemIntegration,
  isDesktopApp,
  openDiagnostics,
  openReleasePage,
  sendDaemonCommand,
  startDaemon,
  type AccountDraft,
  type AccountSummary,
  type DaemonCommand,
  type DaemonSnapshot,
  type GuardPhase,
} from "@/lib/daemon";

type Account = AccountSummary;
type FormState = { mode: "add" | "edit"; id?: string; username: string; verified?: boolean };
type BusyState = { kind: "login" | "switch"; accountId: string } | null;
type WindowSize = "compact" | "standard" | "large";
type WindowRatio = "4:3" | "3:2" | "16:10" | "16:9";
type WindowMetrics = { width: number; height: number };

const WINDOW_SIZES = {
  compact: { label: "小", height: 675 },
  standard: { label: "中", height: 780 },
  large: { label: "大", height: 900 },
} as const;
const WINDOW_RATIOS = {
  "4:3": { label: "4:3", value: 4 / 3 },
  "3:2": { label: "3:2", value: 3 / 2 },
  "16:10": { label: "16:10", value: 16 / 10 },
  "16:9": { label: "16:9", value: 16 / 9 },
} as const;
const WINDOW_SIZE_KEY = "vg-campus-window-size";
const WINDOW_RATIO_KEY = "vg-campus-window-ratio";
const GUARD_INTENT_KEY = "vg-campus-guard-enabled";

function readWindowSize(): WindowSize | null {
  try {
    const stored = localStorage.getItem(WINDOW_SIZE_KEY);
    if (stored === "compact" || stored === "standard" || stored === "large") return stored;
  } catch {
    // Storage can be unavailable in restricted preview environments.
  }
  return null;
}

function readWindowRatio(): WindowRatio | null {
  try {
    const stored = localStorage.getItem(WINDOW_RATIO_KEY);
    if (stored && stored in WINDOW_RATIOS) return stored as WindowRatio;
  } catch {
    // Storage can be unavailable in restricted preview environments.
  }
  return null;
}

function fitWindowMetrics(
  size: WindowSize,
  ratio: WindowRatio,
  maxWidth = 1600,
  maxHeight = 900,
): WindowMetrics {
  const ratioValue = WINDOW_RATIOS[ratio].value;
  let height = Math.min(WINDOW_SIZES[size].height, maxHeight);
  let width = height * ratioValue;
  if (width > maxWidth) {
    width = maxWidth;
    height = width / ratioValue;
  }
  return {
    width: Math.max(900, Math.round(width)),
    height: Math.max(600, Math.round(height)),
  };
}

async function resolveWindowMetrics(size: WindowSize, ratio: WindowRatio): Promise<WindowMetrics> {
  if (!isTauri()) return fitWindowMetrics(size, ratio);
  try {
    const monitor = await currentMonitor();
    if (!monitor) return fitWindowMetrics(size, ratio);
    const availableWidth = monitor.workArea.size.width / monitor.scaleFactor - 48;
    const availableHeight = monitor.workArea.size.height / monitor.scaleFactor - 48;
    return fitWindowMetrics(size, ratio, availableWidth, availableHeight);
  } catch {
    return fitWindowMetrics(size, ratio);
  }
}

async function chooseAdaptiveWindowSize(ratio: WindowRatio): Promise<WindowSize> {
  const large = await resolveWindowMetrics("large", ratio);
  if (large.height >= WINDOW_SIZES.large.height - 2) return "large";
  const standard = await resolveWindowMetrics("standard", ratio);
  if (standard.height >= WINDOW_SIZES.standard.height - 2) return "standard";
  return "compact";
}

const INITIAL_ACCOUNTS: Account[] = [
  { id: "primary", username: "student_01", enabled: true, verified: true, active: true },
  { id: "backup", username: "backup_02", enabled: true, verified: true, active: false },
  { id: "backup3", username: "backup_03", enabled: true, verified: false, active: false },
  { id: "backup4", username: "backup_04", enabled: true, verified: true, active: false },
  { id: "backup5", username: "backup_05", enabled: true, verified: true, active: false },
];

const NO_DRAG = { WebkitAppRegion: "no-drag" } as CSSProperties;
const START_STAGES = [
  { at: 0, label: "正在启动网络守护" },
  { at: 620, label: "正在读取校园网账号" },
  { at: 1380, label: "正在登录校园网" },
  { at: 2100, label: "正在验证网络连接" },
  { at: 2700, label: "网络守护已就绪" },
];
const START_DURATION = 2700;

const PREVIEW_SNAPSHOT: DaemonSnapshot = {
  protocol_version: 1,
  daemon_version: "0.1.2",
  guard_enabled: true,
  auto_start_enabled: true,
  phase: "online",
  online: true,
  active_account_id: "primary",
  accounts: INITIAL_ACCOUNTS,
  portal: {
    host: "beiradius.chd.edu.cn",
    ac_id: "5",
    display_name: "长安大学",
  },
  user: {
    username: "student_01",
    ip: "10.0.0.28",
    mac: "02:00:00:00:00:28",
    online_since: "2026-10-30T08:24:02Z",
    session_seconds: 8292,
    total_bytes: 448279442227,
    balance: 0,
    product_name: "教工",
  },
  metrics: {
    process_cpu_percent: 0.04,
    process_memory_bytes: 12.7 * 1024 * 1024,
    last_probe_latency_ms: 312,
    checks_total: 1428,
    reconnect_total: 3,
  },
  last_check_at: "2026-10-30T10:42:16Z",
  next_retry_at: null,
  last_error: null,
};

const EMPTY_SNAPSHOT: DaemonSnapshot = {
  ...PREVIEW_SNAPSHOT,
  guard_enabled: false,
  auto_start_enabled: false,
  phase: "off",
  online: false,
  active_account_id: null,
  accounts: [],
  portal: null,
  user: null,
  metrics: {
    process_cpu_percent: 0,
    process_memory_bytes: 0,
    last_probe_latency_ms: 0,
    checks_total: 0,
    reconnect_total: 0,
  },
  last_check_at: null,
  next_retry_at: null,
  last_error: null,
};

function phaseLabel(phase: GuardPhase): string {
  switch (phase) {
    case "starting": return "正在启动网络守护";
    case "waiting_network": return "等待校园网连接";
    case "discovering_portal": return "正在识别校园网门户";
    case "authenticating": return "正在登录校园网";
    case "online": return "网络守护已就绪";
    case "suspicious": return "正在复核连接状态";
    case "reconnecting": return "正在恢复校园网连接";
    case "needs_action": return "需要处理";
    case "crashed": return "守护异常停止";
    default: return "网络守护已关闭";
  }
}

function formatBytes(value: number): string {
  if (!value) return "—";
  const gigabytes = value / 1024 ** 3;
  return `${gigabytes.toFixed(2)} GB`;
}

function formatDuration(value: number): string {
  if (!value) return "—";
  const hours = Math.floor(value / 3600);
  const minutes = Math.floor((value % 3600) / 60);
  return `${hours} 小时 ${minutes} 分`;
}

function formatClock(value?: string | null): string {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "—";
  return date.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

function formatDateTime(value?: string | null): string {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "—";
  return date.toLocaleString("zh-CN", { hour12: false });
}

function WindowControls() {
  async function run(action: () => Promise<void>) {
    try {
      await action();
    } catch {
      lensToast.error("窗口控制失败，请稍后重试");
    }
  }

  return (
    <div className="vg-window-controls">
      <LensActionButton label="−" className="vg-window-control" ariaLabel="最小化窗口" onPress={() => void run(() => getCurrentWindow().minimize())} />
      <LensActionButton label="×" className="vg-window-control is-close" ariaLabel="关闭窗口" onPress={() => void run(() => invoke("exit_app"))} />
    </div>
  );
}

function PreviewWindowControls() {
  return (
    <div className="vg-window-controls" aria-hidden="true">
      <span className="vg-window-control"><Minus /></span>
      <span className="vg-window-control is-close"><span className="vg-close-glyph" /></span>
    </div>
  );
}

function Brand() {
  return (
    <div className="vg-brand">
      <span className="vg-brand-mark"><img src={appIcon} alt="" draggable={false} /></span>
      <h1>VG校园网助手</h1>
      <span className="vg-brand-edition">LENS</span>
    </div>
  );
}

function NetworkGuard({
  enabled,
  phase,
  cpuPercent,
  memoryBytes,
  onChange,
  onRepair,
}: {
  enabled: boolean;
  phase: GuardPhase;
  cpuPercent: number;
  memoryBytes: number;
  onChange: (enabled: boolean) => Promise<void>;
  onRepair: () => Promise<void>;
}) {
  const [starting, setStarting] = useState(false);
  const [progressClosing, setProgressClosing] = useState(false);
  const [progress, setProgress] = useState(0);
  const [stage, setStage] = useState(START_STAGES[0].label);
  const progressFrame = useRef<number>();
  const finishTimer = useRef<ReturnType<typeof setTimeout>>();

  useEffect(() => () => {
    if (progressFrame.current) cancelAnimationFrame(progressFrame.current);
    if (finishTimer.current) clearTimeout(finishTimer.current);
  }, []);

  async function changeGuard(next: boolean) {
    if (!next) {
      if (progressFrame.current) cancelAnimationFrame(progressFrame.current);
      if (finishTimer.current) clearTimeout(finishTimer.current);
      setProgressClosing(false);
      setStarting(false);
      try {
        await onChange(false);
      } catch (error) {
        lensToast.error(error instanceof Error ? error.message : "关闭网络守护失败");
      }
      return;
    }
    if (enabled || starting) return;
    if (progressFrame.current) cancelAnimationFrame(progressFrame.current);
    if (finishTimer.current) clearTimeout(finishTimer.current);
    setProgressClosing(false);
    setStarting(true);
    setProgress(4);
    setStage(START_STAGES[0].label);
    const startedAt = performance.now();
    const action = onChange(true);
    const tick = (now: number) => {
      const elapsed = Math.min(now - startedAt, START_DURATION);
      const nextIndex = START_STAGES.findIndex(item => elapsed <= item.at);
      const stageIndex = nextIndex === -1 ? START_STAGES.length - 1 : nextIndex;
      const linear = Math.min(elapsed / START_DURATION, 0.92);
      const eased = linear * linear * (3 - 2 * linear);
      setProgress(4 + 86 * eased);
      setStage(START_STAGES[stageIndex].label);

      if (elapsed < START_DURATION) {
        progressFrame.current = requestAnimationFrame(tick);
      }
    };
    progressFrame.current = requestAnimationFrame(tick);

    try {
      await action;
      if (progressFrame.current) cancelAnimationFrame(progressFrame.current);
      setProgress(100);
      setStage("网络守护已就绪");
      finishTimer.current = setTimeout(() => {
        setProgressClosing(true);
        finishTimer.current = setTimeout(() => {
          setStarting(false);
          setProgressClosing(false);
        }, 220);
      }, 850);
    } catch (error) {
      if (progressFrame.current) cancelAnimationFrame(progressFrame.current);
      setStarting(false);
      setProgress(0);
      lensToast.error(error instanceof Error ? error.message : "启动网络守护失败");
    }
  }

  async function repair() {
    if (starting) return;
    setStarting(true);
    setProgress(12);
    setStage("正在重新启动网络守护");
    try {
      await onRepair();
      setProgress(100);
      setStage("网络守护已就绪");
    } catch (error) {
      setStarting(false);
      lensToast.error(error instanceof Error ? error.message : "重新启动网络守护失败");
      return;
    }
    finishTimer.current = setTimeout(() => {
      setStarting(false);
      setProgressClosing(false);
    }, 600);
  }

  return (
    <LensPanel label="网络守护" className="vg-panel vg-guard-panel">
      <div className="vg-guard-copy">
        {starting ? (
          <div
            className={cn("vg-guard-progress", progressClosing && "is-closing")}
            role="status"
            aria-live="polite"
          >
            <LensProgress
              label="网络守护启动进度"
              statusLabel={stage}
              value={progress}
              animate={progress >= 100}
            />
          </div>
        ) : (
          <div className="vg-guard-normal">
            <div className="vg-guard-heading">
              <span className="vg-guard-lens" data-active={enabled}>
                <LensIconButton
                  label="网络守护连接状态"
                  icon="check"
                  pressed={enabled}
                  decorative
                />
                <ShieldCheck className="vg-guard-shield" aria-hidden="true" />
              </span>
              <h2>网络守护</h2>
            </div>
            <p className="vg-guard-description">
              <span>网络守护会在开机时自动登录校园网</span>
              <span>持续监测连接 断线时自动重连</span>
              <span>资源占用可忽略不计 关闭窗口不受影响</span>
            </p>
            <div className="vg-resource-row" aria-label="守护进程资源占用">
              <span><Cpu /><span>CPU</span><strong className="vg-selectable">{enabled ? `${cpuPercent.toFixed(2)} %` : "—"}</strong></span>
              <span><MemoryStick /><span>内存</span><strong className="vg-selectable">{enabled && memoryBytes ? `${(memoryBytes / 1024 / 1024).toFixed(2)} MB` : "—"}</strong></span>
              <span className="vg-sample-note">{phaseLabel(phase)}</span>
            </div>
          </div>
        )}
      </div>
      <div className="vg-guard-control">
        {phase === "crashed" && !starting ? (
          <LensActionButton
            label="重新启动"
            className="vg-quiet-button"
            onPress={() => void repair()}
          />
        ) : null}
        <LensSwitch
          label="网络守护"
          checked={enabled || starting}
          disabled={starting}
          onChange={changeGuard}
        />
      </div>
    </LensPanel>
  );
}

function AccountRecord({
  account,
  active,
  online,
  index,
  count,
  loggingIn,
  switchDisabled,
  onSwitch,
  onEdit,
  onMove,
  onToggle,
  onRemove,
}: {
  account: Account;
  active: boolean;
  online: boolean;
  index: number;
  count: number;
  loggingIn: boolean;
  switchDisabled: boolean;
  onSwitch: () => void;
  onEdit: () => void;
  onMove: (direction: number) => void;
  onToggle: () => void;
  onRemove: () => void;
}) {
  const [confirmingRemove, setConfirmingRemove] = useState(false);

  useEffect(() => {
    if (active) setConfirmingRemove(false);
  }, [active]);

  return (
    <div className="viz-record vg-account-record" data-selected={active} role="listitem">
      <span className="viz-row-glass lg-surface" data-glass="optical-well" aria-hidden="true" />
      <span className="vg-account-index">{String(index + 1).padStart(2, "0")}</span>
      <span className="vg-account-copy">
        <strong>{account.username}</strong>
        {active ? <span className="vg-current-badge"><Check />当前</span> : null}
        {!account.verified ? <span className="vg-pending-badge">未验证</span> : null}
      </span>
      <span className="vg-account-action-cell">
        <LensChoiceAction
          label={loggingIn ? "连接中" : active ? (online ? "在线" : "当前") : "切换"}
          selected={active}
          disabled={active || !account.enabled || switchDisabled}
          onPress={onSwitch}
        />
        <LensMenu
          label={confirmingRemove ? `确认移除 ${account.username}` : `更多操作 ${account.username}`}
          className={cn("vg-account-menu", confirmingRemove && "vg-confirm-menu")}
          open={confirmingRemove || undefined}
          onOpenChange={open => {
            if (!open && confirmingRemove) setConfirmingRemove(false);
          }}
          options={confirmingRemove
            ? [
                { value: "cancel", label: "取消" },
                { value: "confirm", label: "确认移除" },
              ]
            : [
                { value: "edit", label: "编辑账号" },
                { value: "up", label: "上移一行", disabled: index === 0 },
                { value: "down", label: "下移一行", disabled: index === count - 1 },
                { value: "toggle", label: account.enabled ? "停用账号" : "启用账号" },
                { value: "remove", label: "移除账号", disabled: active },
              ]}
          onAction={value => {
            if (value === "edit") onEdit();
            if (value === "up") onMove(-1);
            if (value === "down") onMove(1);
            if (value === "toggle") onToggle();
            if (value === "remove") setConfirmingRemove(true);
            if (value === "cancel") setConfirmingRemove(false);
            if (value === "confirm") {
              setConfirmingRemove(false);
              onRemove();
            }
          }}
        />
      </span>
    </div>
  );
}

function DataField({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="vg-data-field">
      <dt>{label}</dt>
      <dd className="vg-selectable">{children}</dd>
    </div>
  );
}

function AnimatedValue({ value, className }: { value: string; className?: string }) {
  const ref = useRef<HTMLElement>(null);
  const initial = useRef(value);

  useEffect(() => {
    animateTextChange(ref.current, value);
  }, [value]);

  return <strong ref={ref} className={className}>{initial.current}</strong>;
}

function AccountDialog({
  form,
  onClose,
  onSave,
}: {
  form: FormState | null;
  onClose: () => void;
  onSave: (username: string, password: string) => Promise<void>;
}) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setUsername(form?.username ?? "");
    setPassword("");
    setShowPassword(false);
    setError("");
    setSaving(false);
  }, [form]);

  async function submit() {
    if (!username.trim()) {
      setError("请输入用户名");
      return;
    }
    if (!password && form?.mode === "add") {
      setError("请输入密码");
      return;
    }
    setSaving(true);
    setError("");
    try {
      await onSave(username.trim(), password);
      setPassword("");
    } catch (saveError) {
      setError(saveError instanceof Error ? saveError.message : "保存账号失败");
    } finally {
      setSaving(false);
    }
  }

  return (
    <LensDialog
      open={Boolean(form)}
      onClose={onClose}
      title={form?.mode === "edit" ? "编辑账号" : "添加账号"}
      footer={(
        <>
          <LensActionButton label="取消" onPress={onClose} />
          <LensActionButton label={saving ? "正在保存" : form?.mode === "edit" ? "保存修改" : "添加账号"} variant="success" disabled={saving} onPress={() => void submit()} />
        </>
      )}
    >
      <form
        className="vg-form"
        onSubmit={event => {
          event.preventDefault();
          void submit();
        }}
      >
        <label className="vg-form-label">
          用户名
          <LensSearchField
            label="用户名"
            value={username}
            placeholder="校园网用户名"
            autoComplete="username"
            onChange={value => {
              setUsername(value);
              if (error) setError("");
            }}
          />
        </label>
        <label className="vg-form-label">
          密码
          <span className="vg-password-field">
            <LensSearchField
              label="密码"
              value={password}
              type={showPassword ? "text" : "password"}
              placeholder={form?.mode === "edit" ? "留空可保留原密码" : "校园网密码"}
              autoComplete={form?.mode === "edit" ? "current-password" : "new-password"}
              onChange={value => {
                setPassword(value);
                if (error) setError("");
              }}
            />
            <LensActionButton
              label={showPassword ? "隐藏" : "显示"}
              className="vg-password-toggle"
              onPress={() => setShowPassword(value => !value)}
            />
          </span>
        </label>
        {error ? <p className="vg-form-error" role="alert">{error}</p> : null}
        <p className="vg-form-hint"><LockKeyhole />密码仅在本机加密保存，不会上传</p>
      </form>
    </LensDialog>
  );
}

function PortalDialog({
  open,
  currentHost,
  currentAcId,
  onClose,
  onSaved,
}: {
  open: boolean;
  currentHost?: string;
  currentAcId?: string;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const currentUrl = currentHost
    ? `https://${currentHost}/srun_portal_pc?ac_id=${encodeURIComponent(currentAcId || "5")}&theme=pro`
    : "";

  useEffect(() => {
    if (!open) return;
    setUrl(currentUrl);
    setBusy(false);
    setError("");
  }, [open]);

  async function detect() {
    setBusy(true);
    setError("");
    try {
      const portal = await discoverPortal();
      setUrl(`${portal.base_url}/srun_portal_pc?ac_id=${encodeURIComponent(portal.ac_id)}&theme=pro`);
    } catch (detectError) {
      if (currentUrl) {
        setUrl(currentUrl);
        setError("当前已在线时通常不会发生门户跳转，已保留现有门户地址");
      } else {
        setError(detectError instanceof Error ? detectError.message : "没有自动发现校园网门户");
      }
    } finally {
      setBusy(false);
    }
  }

  async function save() {
    if (!url.trim()) {
      setError("请粘贴浏览器中的校园网登录页地址");
      return;
    }
    setBusy(true);
    setError("");
    try {
      await startDaemon();
      const snapshot = await sendDaemonCommand({ type: "set_portal", portal_url: url.trim() });
      if (!snapshot) throw new Error("守护没有返回门户配置");
      onSaved();
    } catch (saveError) {
      setError(saveError instanceof Error ? saveError.message : "保存校园网门户失败");
    } finally {
      setBusy(false);
    }
  }

  return (
    <LensDialog
      open={open}
      onClose={onClose}
      title="校园网门户"
      description="首次连接自动识别；失败时从浏览器复制登录页地址。"
      width={500}
      footer={(
        <>
          <LensActionButton label="取消" onPress={onClose} />
          <LensActionButton
            label={busy ? "正在处理" : "保存门户"}
            variant="success"
            disabled={busy}
            onPress={() => void save()}
          />
        </>
      )}
    >
      <div className="vg-form">
        {currentHost ? (
          <p className="vg-form-hint"><Globe2 />当前门户：{currentHost}</p>
        ) : null}
        <LensActionButton
          label={busy ? "正在识别" : "自动识别校园网门户"}
          className="vg-detect-portal"
          disabled={busy}
          onPress={() => void detect()}
        />
        <label className="vg-form-label">
          登录页地址
          <LensSearchField
            label="校园网登录页地址"
            value={url}
            placeholder="https://portal.example.edu.cn/srun_portal_pc?ac_id=5"
            onChange={value => {
              setUrl(value);
              if (error) setError("");
            }}
          />
        </label>
        <p className="vg-form-hint"><LockKeyhole />认证只通过 HTTPS 提交，不会向 HTTP 页面发送账号密码。</p>
        {error ? <p className="vg-form-error" role="alert">{error}</p> : null}
      </div>
    </LensDialog>
  );
}

function SwitchAccountDialog({
  account,
  onCancel,
  onConfirm,
}: {
  account: Account | null;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <LensDialog
      open={Boolean(account)}
      onClose={onCancel}
      title="切换登录账号"
      description={account ? `将先执行一键下线，再登录 ${account.username}。` : ""}
      footer={(
        <>
          <LensActionButton label="取消" onPress={onCancel} />
          <LensActionButton label="确认切换" variant="success" onPress={onConfirm} />
        </>
      )}
    >
      <p className="vg-form-hint">切换过程会短暂断网；如果目标账号登录失败，守护会尝试恢复原账号。</p>
    </LensDialog>
  );
}

export default function LensOriginalApp() {
  const desktopApp = isDesktopApp();
  const initialSnapshot = desktopApp ? EMPTY_SNAPSHOT : PREVIEW_SNAPSHOT;
  const [accounts, setAccounts] = useState<Account[]>(initialSnapshot.accounts);
  const [activeId, setActiveId] = useState(initialSnapshot.active_account_id ?? "");
  const [online, setOnline] = useState(initialSnapshot.online);
  const [busy, setBusy] = useState<BusyState>(null);
  const [guard, setGuard] = useState(initialSnapshot.guard_enabled);
  const [phase, setPhase] = useState<GuardPhase>(initialSnapshot.phase);
  const [portal, setPortal] = useState(initialSnapshot.portal);
  const [user, setUser] = useState(initialSnapshot.user);
  const [metrics, setMetrics] = useState(initialSnapshot.metrics);
  const [lastCheckAt, setLastCheckAt] = useState(initialSnapshot.last_check_at);
  const [lastError, setLastError] = useState(initialSnapshot.last_error);
  const [daemonConnected, setDaemonConnected] = useState(!desktopApp);
  const [form, setForm] = useState<FormState | null>(null);
  const [portalOpen, setPortalOpen] = useState(false);
  const [pendingSwitch, setPendingSwitch] = useState<Account | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [windowSize, setWindowSize] = useState<WindowSize>("compact");
  const [windowRatio, setWindowRatio] = useState<WindowRatio>("16:10");
  const [windowMetrics, setWindowMetrics] = useState<WindowMetrics>(() => (
    fitWindowMetrics("compact", "16:10")
  ));
  const [homeView, setHomeView] = useState<"connection" | "detail">("connection");
  const [accountPage, setAccountPage] = useState(0);
  const [accountsPerPage, setAccountsPerPage] = useState(5);
  const accountBoardRef = useRef<HTMLDivElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>();
  const onlineRef = useRef(online);
  const busyRef = useRef<BusyState>(busy);
  const activeRef = useRef<Account | undefined>();
  const lastErrorRef = useRef<string | null>(null);
  const { theme, setTheme } = useTheme();
  const active = accounts.find(account => account.id === activeId);
  const totalPages = Math.max(1, Math.ceil(accounts.length / accountsPerPage));
  const visiblePage = Math.min(accountPage, totalPages - 1);
  const visibleAccounts = accounts.slice(
    visiblePage * accountsPerPage,
    visiblePage * accountsPerPage + accountsPerPage,
  );
  const scene = theme === "dark" ? "ocean" : "aurora";

  useEffect(() => () => clearTimeout(timer.current), []);
  useEffect(() => {
    if (!desktopApp) return;
    let disposed = false;
    async function refresh() {
      try {
        const snapshot = await getDaemonSnapshot();
        if (!disposed) applySnapshot(snapshot);
      } catch {
        if (!disposed) {
          setDaemonConnected(false);
          try {
            if (localStorage.getItem(GUARD_INTENT_KEY) === "true") {
              setGuard(true);
              setPhase("crashed");
            }
          } catch {
            // State persistence is optional in restricted previews.
          }
        }
      }
    }
    void refresh();
    const poll = setInterval(() => {
      if (!document.hidden) void refresh();
    }, 2000);
    return () => {
      disposed = true;
      clearInterval(poll);
    };
  }, [desktopApp]);
  useEffect(() => {
    if (!isTauri()) return;
    let active = true;
    void (async () => {
      const ratio = readWindowRatio() ?? "16:10";
      const size = readWindowSize() ?? await chooseAdaptiveWindowSize(ratio);
      const metrics = await resolveWindowMetrics(size, ratio);
      if (!active) return;
      setWindowSize(size);
      setWindowRatio(ratio);
      setWindowMetrics(metrics);
      const win = getCurrentWindow();
      try {
        await win.setSize(new LogicalSize(metrics.width, metrics.height));
        await win.center();
      } catch {
        lensToast.error("无法应用窗口大小");
      }
    })();
    return () => {
      active = false;
    };
  }, []);
  useEffect(() => {
    onlineRef.current = online;
  }, [online]);
  useEffect(() => {
    busyRef.current = busy;
  }, [busy]);
  useEffect(() => {
    activeRef.current = active;
  }, [active]);
  useEffect(() => {
    if (!lastError || lastError === lastErrorRef.current) return;
    lastErrorRef.current = lastError;
    lensToast.error(lastError);
  }, [lastError]);
  useEffect(() => {
    const board = accountBoardRef.current;
    if (!board) return;
    let frame = 0;
    const measure = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        const list = board.querySelector<HTMLElement>(".vg-account-list");
        const row = board.querySelector<HTMLElement>(".vg-account-record");
        const rowHeight = row?.offsetHeight ?? (window.innerHeight <= 680 ? 44 : 48);
        const gap = Number.parseFloat(getComputedStyle(list ?? board).rowGap) || 7;
        const available = board.clientHeight;
        if (!available) return;
        const capacity = Math.max(1, Math.floor((available + gap) / (rowHeight + gap)));
        setAccountsPerPage(current => current === capacity ? current : capacity);
        setAccountPage(current => Math.min(
          current,
          Math.max(0, Math.ceil(accounts.length / capacity) - 1),
        ));
      });
    };
    const observer = new ResizeObserver(measure);
    observer.observe(board);
    window.addEventListener("resize", measure, { passive: true });
    measure();
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", measure);
      cancelAnimationFrame(frame);
    };
  }, [accounts.length, homeView]);

  function applySnapshot(snapshot: DaemonSnapshot) {
    setAccounts(snapshot.accounts);
    setActiveId(snapshot.active_account_id ?? snapshot.accounts.find(account => account.active)?.id ?? "");
    setGuard(snapshot.guard_enabled);
    setPhase(snapshot.phase);
    setOnline(snapshot.online);
    setPortal(snapshot.portal);
    setUser(snapshot.user);
    setMetrics(snapshot.metrics);
    setLastCheckAt(snapshot.last_check_at);
    setLastError(snapshot.last_error);
    setDaemonConnected(true);
    onlineRef.current = snapshot.online;
    try {
      localStorage.setItem(GUARD_INTENT_KEY, String(snapshot.guard_enabled));
    } catch {
      // State persistence is optional in restricted previews.
    }
  }

  async function executeCommand(command: DaemonCommand): Promise<DaemonSnapshot> {
    const snapshot = await sendDaemonCommand(command);
    if (!snapshot) throw new Error("校园网守护没有返回状态");
    applySnapshot(snapshot);
    return snapshot;
  }

  async function handleGuardChange(next: boolean) {
    if (!desktopApp) {
      await new Promise(resolve => setTimeout(resolve, 650));
      setGuard(next);
      setPhase(next ? "online" : "off");
      setOnline(next);
      if (!next) setUser(null);
      return;
    }

    try {
      await startDaemon();
      const snapshot = await getDaemonSnapshot();
      applySnapshot(snapshot);
      if (next && !snapshot.portal) {
        setPortalOpen(true);
        throw new Error("请先完成校园网门户配置");
      }
      if (next && snapshot.accounts.length === 0) {
        setForm({ mode: "add", id: crypto.randomUUID(), username: "" });
        throw new Error("请先添加校园网登录账号");
      }
      let autoStartWarning: string | null = null;
      if (next && !snapshot.auto_start_enabled) {
        lensToast.info("首次开启需要在 Windows 权限提示中允许注册开机守护");
        try {
          await installSystemIntegration();
          applySnapshot(await getDaemonSnapshot());
        } catch (installationError) {
          autoStartWarning = installationError instanceof Error
            ? installationError.message
            : "系统开机启动项注册失败";
          setLastError(autoStartWarning);
        }
      }
      await executeCommand({ type: "set_guard", enabled: next });
      if (!next) {
        await executeCommand({ type: "shutdown" });
        setDaemonConnected(false);
      }
      try {
        localStorage.setItem(GUARD_INTENT_KEY, String(next));
      } catch {
        // State persistence is optional in restricted previews.
      }
      if (autoStartWarning) {
        lensToast.error(`开机启动项未注册，当前会话将继续尝试守护：${autoStartWarning}`);
      }
      lensToast.success(next ? "网络守护已启动" : "网络守护已关闭");
    } catch (error) {
      const message = error instanceof Error ? error.message : "校园网守护操作失败";
      setLastError(message);
      throw error;
    }
  }

  async function repairGuard() {
    await handleGuardChange(false);
    await handleGuardChange(true);
  }

  async function handleAccountSave(username: string, password: string) {
    if (!form) return;
    if (!desktopApp) {
      if (form.mode === "edit" && form.id) {
        setAccounts(previous => previous.map(account => (
          account.id === form.id ? { ...account, username, verified: false } : account
        )));
      } else {
        const id = crypto.randomUUID();
        setAccounts(previous => [
          ...previous,
          { id, username, enabled: true, verified: false, active: previous.length === 0 },
        ]);
        if (!activeRef.current) setActiveId(id);
      }
      setForm(null);
      lensToast.success("账号已保存");
      return;
    }

    await startDaemon();
    const account: AccountDraft = {
      id: form.id ?? crypto.randomUUID(),
      username,
      password: password || undefined,
      enabled: form.mode === "edit"
        ? accounts.find(entry => entry.id === form.id)?.enabled ?? true
        : true,
    };
    await executeCommand({ type: "save_account", account });
    setForm(null);
    lensToast.success(form.mode === "edit" ? "账号修改已保存" : "账号已保存，使用时自动验证");
  }

  function handlePortalSaved() {
    setPortalOpen(false);
    void getDaemonSnapshot().then(applySnapshot).catch(() => undefined);
    lensToast.success("校园网门户已保存");
    if (accounts.length === 0) {
      setForm({ mode: "add", id: crypto.randomUUID(), username: "" });
    }
  }

  async function changeWindowLayout(size: WindowSize, ratio: WindowRatio) {
    const metrics = await resolveWindowMetrics(size, ratio);
    setWindowSize(size);
    setWindowRatio(ratio);
    setWindowMetrics(metrics);
    try {
      localStorage.setItem(WINDOW_SIZE_KEY, size);
      localStorage.setItem(WINDOW_RATIO_KEY, ratio);
    } catch {
      // Persisting window preferences is optional.
    }
    if (!isTauri()) return;
    try {
      const win = getCurrentWindow();
      await win.setSize(new LogicalSize(metrics.width, metrics.height));
      await win.center();
    } catch {
      lensToast.error("无法调整窗口大小");
    }
  }

  function changeWindowSize(value: string) {
    if (value !== "compact" && value !== "standard" && value !== "large") return;
    void changeWindowLayout(value, windowRatio);
  }

  function changeWindowRatio(value: string) {
    if (value !== "4:3" && value !== "3:2" && value !== "16:10" && value !== "16:9") return;
    void changeWindowLayout(windowSize, value);
  }

  function switchAccount(account: Account) {
    if (busyRef.current || !account.enabled || account.id === activeId) return;
    setPendingSwitch(account);
  }

  async function confirmSwitch() {
    const account = pendingSwitch;
    if (!account) return;
    setPendingSwitch(null);
    if (!desktopApp) {
      setActiveId(account.id);
      return;
    }
    const nextBusy: BusyState = { kind: "switch", accountId: account.id };
    busyRef.current = nextBusy;
    setBusy(nextBusy);
    try {
      await executeCommand({ type: "switch_account", id: account.id });
      lensToast.success(`已切换到 ${account.username}`);
    } catch (error) {
      lensToast.error(error instanceof Error ? error.message : "账号切换失败");
    } finally {
      busyRef.current = null;
      setBusy(null);
    }
  }

  async function moveAccount(id: string, direction: number) {
    const next = [...accounts];
    const index = next.findIndex(account => account.id === id);
    const target = index + direction;
    if (index < 0 || target < 0 || target >= next.length) return;
    [next[index], next[target]] = [next[target], next[index]];
    if (!desktopApp) {
      setAccounts(next);
      return;
    }
    try {
      await executeCommand({ type: "reorder_accounts", ids: next.map(account => account.id) });
    } catch (error) {
      lensToast.error(error instanceof Error ? error.message : "账号排序失败");
    }
  }

  async function removeAccount(id: string) {
    if (!desktopApp) {
      const remaining = accounts.filter(account => account.id !== id);
      setAccounts(remaining);
      if (activeId === id) setActiveId(remaining.find(account => account.enabled)?.id ?? "");
      lensToast.success("账号移除成功");
      return;
    }
    try {
      await executeCommand({ type: "remove_account", id });
      lensToast.success("账号移除成功");
    } catch (error) {
      lensToast.error(error instanceof Error ? error.message : "账号移除失败");
    }
  }

  async function toggleAccount(account: Account) {
    if (!desktopApp) {
      setAccounts(previous => previous.map(entry => (
        entry.id === account.id ? { ...entry, enabled: !entry.enabled } : entry
      )));
      return;
    }
    try {
      await executeCommand({
        type: "set_account_enabled",
        id: account.id,
        enabled: !account.enabled,
      });
      lensToast.success(`${account.username} 已${account.enabled ? "停用" : "启用"}`);
    } catch (error) {
      lensToast.error(error instanceof Error ? error.message : "账号状态更新失败");
    }
  }

  async function checkNow() {
    if (!desktopApp) return;
    try {
      await executeCommand({ type: "check_now" });
      lensToast.info("已安排立即检测");
    } catch (error) {
      lensToast.error(error instanceof Error ? error.message : "无法立即检测");
    }
  }

  return (
    <LensStage scene={scene} className="vg-stage">
      <div className="vg-window">
        <header
          className="vg-titlebar"
          {...DRAG_REGION_ATTR}
          style={DRAG_REGION_STYLE as CSSProperties}
        >
          <Brand />
          <div className="vg-titlebar-spacer" />
          <div className="vg-titlebar-actions" style={NO_DRAG}>
            <LensIconButton
              label={theme === "dark" ? "切换到流光" : "切换到深海"}
              icon={theme === "dark" ? "sun" : "moon"}
              className="vg-theme-toggle"
              onPress={() => setTheme(theme === "dark" ? "light" : "dark")}
            />
            <LensIconButton label="设置" icon="adjust" className="vg-toolbar-icon" onPress={() => setSettingsOpen(true)} />
            <span className="vg-toolbar-separator" />
            {isTauri() ? <WindowControls /> : <PreviewWindowControls />}
          </div>
        </header>

        <main className="vg-main">
          {homeView === "connection" ? (
            <>
              <NetworkGuard
                enabled={guard}
                phase={!daemonConnected && guard ? "crashed" : phase}
                cpuPercent={metrics.process_cpu_percent}
                memoryBytes={metrics.process_memory_bytes}
                onChange={handleGuardChange}
                onRepair={repairGuard}
              />

              <div className="vg-home-grid">
                <div className="vg-left-stack">
                  <LensPanel label="校园网连接" className="vg-panel vg-connection-panel">
                    <div className="vg-panel-heading">
                      <h2>校园网连接</h2>
                      <span
                        className={cn("vg-inline-status", busy && "is-visible")}
                        aria-live="polite"
                        aria-hidden={!busy}
                      >
                        正在同步
                      </span>
                    </div>
                    <LensToggleTile
                      label="网络连接"
                      icon="wifi"
                      checked={online}
                      onLabel={user?.username ? `${user.username} · 已登录校园网` : active ? `${active.username} · 已登录校园网` : "已登录校园网"}
                      offLabel={busy ? (busy.kind === "switch" ? "正在切换账号" : "正在登录") : phaseLabel(phase)}
                      className="vg-connection-tile"
                      disabled
                      onChange={() => undefined}
                    />
                    <div className="vg-session-row">
                      <span><small>上线时间</small><AnimatedValue value={online ? formatClock(user?.online_since) : "—"} /></span>
                      <span><small>已在线</small><AnimatedValue value={online ? formatDuration(user?.session_seconds ?? 0) : "—"} /></span>
                    </div>
                  </LensPanel>

                  <LensPanel label="账号信息" className="vg-panel vg-info-panel">
                    <div className="vg-panel-heading">
                      <h2>账号信息</h2>
                      <LensActionButton label="详情" className="vg-quiet-button vg-info-action" onPress={() => setHomeView("detail")} />
                    </div>
                    <dl className="vg-summary-grid">
                      <DataField label="已用流量">{formatBytes(user?.total_bytes ?? 0)}</DataField>
                      <DataField label="已用时长">{formatDuration(user?.session_seconds ?? 0)}</DataField>
                      <DataField label="账号余额">{user?.balance == null ? "—" : <>{user.balance.toFixed(2)} <small>元</small></>}</DataField>
                      <DataField label="产品名称">{user?.product_name || "—"}</DataField>
                    </dl>
                    <div className="vg-info-footer">
                      <span>最近更新 · {formatClock(lastCheckAt)}</span>
                      <LensActionButton label="立即检测" className="vg-quiet-button vg-info-action" disabled={!desktopApp || !guard} onPress={() => void checkNow()} />
                    </div>
                  </LensPanel>
                </div>

                <LensPanel label="登录账号" className="vg-panel vg-accounts-panel">
                  <div className="vg-panel-heading vg-accounts-heading">
                    <div>
                      <h2>登录账号</h2>
                      <span>{accounts.length} 个</span>
                    </div>
                    <LensActionButton
                      label="＋ 添加账号"
                      variant="success"
                      className="vg-add-account"
                      onPress={() => setForm({ mode: "add", id: crypto.randomUUID(), username: "" })}
                    />
                  </div>
                  <div
                    ref={accountBoardRef}
                    className="viz-records-board vg-account-board"
                    data-accounts-per-page={accountsPerPage}
                  >
                    <div className="vg-account-list" role="list" aria-label="校园网登录账号记录">
                      {visibleAccounts.map(account => {
                        const index = accounts.findIndex(entry => entry.id === account.id);
                        return (
                          <AccountRecord
                            key={account.id}
                            account={account}
                            active={account.id === activeId}
                            online={online}
                            index={index}
                            count={accounts.length}
                            loggingIn={busy?.accountId === account.id}
                            switchDisabled={Boolean(busy)}
                            onSwitch={() => switchAccount(account)}
                            onEdit={() => setForm({ mode: "edit", id: account.id, username: account.username, verified: account.verified })}
                            onMove={direction => moveAccount(account.id, direction)}
                            onToggle={() => void toggleAccount(account)}
                            onRemove={() => void removeAccount(account.id)}
                          />
                        );
                      })}
                    </div>
                    {!visibleAccounts.length ? <div className="vg-empty-accounts"><Users /><span>还没有登录账号</span></div> : null}
                  </div>
                  <div className="vg-account-footer">
                    {totalPages > 1 ? (
                      <div className="vg-pager">
                        <LensActionButton label="上一页" className="vg-pager-button" disabled={visiblePage === 0} onPress={() => setAccountPage(visiblePage - 1)} />
                        <span>{visiblePage + 1} / {totalPages}</span>
                        <LensActionButton label="下一页" className="vg-pager-button" disabled={visiblePage >= totalPages - 1} onPress={() => setAccountPage(visiblePage + 1)} />
                      </div>
                    ) : <span>按列表顺序自动尝试</span>}
                  </div>
                </LensPanel>
              </div>
            </>
          ) : (
            <LensPanel label="账号详情" className="vg-panel vg-detail-panel">
              <div className="vg-detail-heading">
                <div>
                  <LensActionButton label="返回网络守护" className="vg-quiet-button vg-back-button" onPress={() => setHomeView("connection")} />
                  <h2>{active?.username ?? "未添加账号"}</h2>
                  <p>当前在线账号的门户记录</p>
                </div>
                <span className="vg-current-badge">{online ? "在线" : phaseLabel(phase)}</span>
              </div>
              <div className="viz-records-board vg-detail-board">
                <table className="viz-table vg-detail-table">
                  <thead><tr><th>字段</th><th>记录</th><th>字段</th><th>记录</th></tr></thead>
                  <tbody>
                    <tr className="viz-record"><td>用户账号<span className="viz-row-glass lg-surface" data-glass="optical-well" aria-hidden="true" /></td><td><strong>{user?.username || active?.username || "—"}</strong></td><td>已用流量</td><td><strong>{formatBytes(user?.total_bytes ?? 0)}</strong></td></tr>
                    <tr className="viz-record"><td>已用时长<span className="viz-row-glass lg-surface" data-glass="optical-well" aria-hidden="true" /></td><td><strong>{formatDuration(user?.session_seconds ?? 0)}</strong></td><td>账号余额</td><td><strong>{user?.balance == null ? "—" : `${user.balance.toFixed(2)} 元`}</strong></td></tr>
                    <tr className="viz-record"><td>IP 地址<span className="viz-row-glass lg-surface" data-glass="optical-well" aria-hidden="true" /></td><td><strong>{user?.ip || "—"}</strong></td><td>产品名称</td><td><strong>{user?.product_name || "—"}</strong></td></tr>
                    <tr className="viz-record"><td>上线时间<span className="viz-row-glass lg-surface" data-glass="optical-well" aria-hidden="true" /></td><td><strong>{formatDateTime(user?.online_since)}</strong></td><td>MAC 地址</td><td><strong>{user?.mac || "—"}</strong></td></tr>
                    <tr className="viz-record"><td>请求次数<span className="viz-row-glass lg-surface" data-glass="optical-well" aria-hidden="true" /></td><td><strong>{metrics.checks_total}</strong></td><td>重连次数</td><td><strong>{metrics.reconnect_total}</strong></td></tr>
                    <tr className="viz-record"><td>最近检测<span className="viz-row-glass lg-surface" data-glass="optical-well" aria-hidden="true" /></td><td><strong>{formatDateTime(lastCheckAt)}</strong></td><td>检测延迟</td><td><strong>{metrics.last_probe_latency_ms ? `${metrics.last_probe_latency_ms} ms` : "—"}</strong></td></tr>
                  </tbody>
                </table>
              </div>
            </LensPanel>
          )}
        </main>

        <footer className="vg-footer">
          <span><Globe2 />{desktopApp ? (daemonConnected ? "后台守护已连接" : "后台守护未运行") : "界面预览"}<span className="vg-footer-dot" />{desktopApp ? (portal?.host ?? "未配置门户") : "演示数据"}</span>
          <span>VG校园网助手 <b>0.1.2</b></span>
        </footer>
      </div>

      <AccountDialog
        form={form}
        onClose={() => setForm(null)}
        onSave={handleAccountSave}
      />

      <PortalDialog
        open={portalOpen}
        currentHost={portal?.host}
        currentAcId={portal?.ac_id}
        onClose={() => setPortalOpen(false)}
        onSaved={handlePortalSaved}
      />

      <SwitchAccountDialog
        account={pendingSwitch}
        onCancel={() => setPendingSwitch(null)}
        onConfirm={() => void confirmSwitch()}
      />

      <LensDialog
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        title="设置"
      >
        <div className="vg-settings">
          <div className="vg-window-metrics" aria-live="polite">
            <strong>{windowMetrics.width} × {windowMetrics.height}</strong>
            <span>{WINDOW_RATIOS[windowRatio].label}</span>
          </div>
          <div className="vg-setting-row vg-window-size-row">
            <div>
              <strong>窗口大小</strong>
              <span>按当前显示器自动限制</span>
            </div>
            <LensTabs
              label="窗口大小"
              value={windowSize}
              className="vg-window-size-tabs"
              options={(Object.entries(WINDOW_SIZES) as Array<[WindowSize, { label: string; height: number }]>)
                .map(([value, preset]) => ({ value, label: preset.label }))}
              onChange={changeWindowSize}
            />
          </div>
          <div className="vg-setting-row vg-window-size-row">
            <div>
              <strong>窗口比例</strong>
              <span>可以逐项测试布局</span>
            </div>
            <LensTabs
              label="窗口比例"
              value={windowRatio}
              className="vg-window-size-tabs vg-window-ratio-tabs"
              options={(Object.entries(WINDOW_RATIOS) as Array<[WindowRatio, { label: string; value: number }]>)
                .map(([value, ratio]) => ({ value, label: ratio.label }))}
              onChange={changeWindowRatio}
            />
          </div>
          <div className="vg-setting-divider" />
          <div className="vg-setting-row">
            <div>
              <strong>校园网门户</strong>
              <span>{portal ? `${portal.host} · ac_id ${portal.ac_id}` : "尚未配置"}</span>
            </div>
            <LensActionButton
              label="配置门户"
              className="vg-quiet-button"
              onPress={() => {
                setSettingsOpen(false);
                setPortalOpen(true);
              }}
            />
          </div>
          <div className="vg-setting-row">
            <div>
              <strong>后台守护</strong>
              <span>{daemonConnected ? `已连接 · 请求 ${metrics.checks_total} 次 · 重连 ${metrics.reconnect_total} 次` : "未运行"}</span>
            </div>
            <LensActionButton
              label={guard ? "立即检测" : "启动守护"}
              className="vg-quiet-button"
              disabled={!desktopApp}
              onPress={() => {
                if (guard) void checkNow();
                else void handleGuardChange(true).catch(() => undefined);
              }}
            />
          </div>
          {lastError ? (
            <div className="vg-setting-row">
              <div>
                <strong>最近错误</strong>
                <span>{lastError}</span>
              </div>
            </div>
          ) : null}
          <div className="vg-setting-row">
            <div>
              <strong>运行记录</strong>
              <span>只保留状态变化和错误，日志内容已脱敏</span>
            </div>
            <LensActionButton
              label="打开日志目录"
              className="vg-quiet-button"
              disabled={!desktopApp}
              onPress={() => void openDiagnostics().catch(() => lensToast.error("无法打开日志目录"))}
            />
          </div>
          <div className="vg-setting-divider" />
          <div className="vg-setting-row">
            <div>
              <strong>版本更新</strong>
              <span>当前版本 0.1.2 · 不进行后台更新检查</span>
            </div>
            <LensActionButton
              label="查看新版本"
              className="vg-quiet-button"
              disabled={!desktopApp}
              onPress={() => void openReleasePage().catch(() => lensToast.error("无法打开 Releases 页面"))}
            />
          </div>
          <div className="vg-setting-divider" />
          <div className="vg-system-credit">
            <div><strong>Lens Design System</strong><span>原始组件 · WebGL 折射 · 弹性与交互音效</span></div>
            <a href="https://github.com/ruiqichenbiec/design-systems" target="_blank" rel="noopener noreferrer">1.1.0 <ExternalLink /></a>
          </div>
          <div className="vg-component-list">
            <span>progress</span><span>switch</span><span>choices</span><span>toggle-tile</span><span>panel</span><span>menu</span>
          </div>
        </div>
      </LensDialog>

      <LensNotificationHost />
    </LensStage>
  );
}
