import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import {
  createComponent,
  createGlassSystem,
  type ButtonHandle,
  type ComponentHandle,
  type DropOption,
  type GlassSystem,
  type MenuHandle,
  type OpticalIcon,
  type OpticalProgressHandle,
  type SceneName,
  type ToggleHandle,
  type ValueHandle,
} from "@/vendor/lens-design-system/index.js";
import { cn } from "@/lib/utils";
import auroraBackground from "@/assets/backgrounds/vg-aurora.jpg";
import oceanBackground from "@/assets/backgrounds/vg-ocean.jpg";

type LensRuntime = {
  systemRef: RefObject<GlassSystem | null>;
};

type RuntimeJelly = {
  bodies: Map<HTMLElement, unknown>;
  reflow: () => void;
};

const LensRuntimeContext = createContext<LensRuntime | null>(null);

function useLensRuntime() {
  const runtime = useContext(LensRuntimeContext);
  if (!runtime) throw new Error("Lens components must be mounted inside LensStage");
  return runtime;
}

function mountWithRuntime(runtime: LensRuntime, handle: ComponentHandle) {
  const system = runtime.systemRef.current;
  if (!system) return;
  system.mount(handle.element);
  system.renderer?.invalidate(500, true);
  handle.element.dispatchEvent(new CustomEvent("lg:sync", { bubbles: true }));
}

function assignClass(element: HTMLElement, className?: string) {
  if (className) element.classList.add(...className.split(/\s+/).filter(Boolean));
}

const textTransitions = new WeakMap<HTMLElement, symbol>();
const backgroundImages = {
  aurora: new Image(),
  ocean: new Image(),
};
backgroundImages.aurora.src = auroraBackground;
backgroundImages.ocean.src = oceanBackground;

function drawBackgroundImage(
  context: CanvasRenderingContext2D,
  image: HTMLImageElement,
  width: number,
  height: number,
) {
  if (!image.complete || !image.naturalWidth || !image.naturalHeight) return false;
  const scale = Math.max(width / image.naturalWidth, height / image.naturalHeight);
  const drawWidth = image.naturalWidth * scale;
  const drawHeight = image.naturalHeight * scale;
  context.drawImage(
    image,
    (width - drawWidth) / 2,
    (height - drawHeight) / 2,
    drawWidth,
    drawHeight,
  );
  return true;
}

export function animateTextChange(element: HTMLElement | null, next: string) {
  if (!element || element.textContent === next) return;
  const token = Symbol();
  textTransitions.set(element, token);
  element.getAnimations().forEach(animation => animation.cancel());

  if (matchMedia("(prefers-reduced-motion: reduce)").matches) {
    element.textContent = next;
    return;
  }

  const restingOpacity = Number.parseFloat(getComputedStyle(element).opacity) || 1;
  const fadeOut = element.animate(
    [
      { opacity: restingOpacity, transform: "translateY(0)" },
      { opacity: 0, transform: "translateY(-3px)" },
    ],
    { duration: 120, easing: "cubic-bezier(0.4, 0, 1, 1)", fill: "forwards" },
  );
  fadeOut.finished.then(() => {
    if (textTransitions.get(element) !== token || !element.isConnected) return;
    fadeOut.cancel();
    element.textContent = next;
    const fadeIn = element.animate(
      [
        { opacity: 0, transform: "translateY(3px)" },
        { opacity: restingOpacity, transform: "translateY(0)" },
      ],
      { duration: 220, easing: "cubic-bezier(0.16, 1, 0.3, 1)" },
    );
    fadeIn.finished.finally(() => {
      if (textTransitions.get(element) === token) textTransitions.delete(element);
    });
  }).catch(() => undefined);
}

export function LensStage({
  scene,
  className,
  children,
}: {
  scene: SceneName;
  className?: string;
  children: ReactNode;
}) {
  const stageRef = useRef<HTMLDivElement>(null);
  const systemRef = useRef<GlassSystem | null>(null);
  const runtime = useMemo(() => ({ systemRef }), []);
  const sceneRef = useRef(scene);
  sceneRef.current = scene;

  useEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    const system = createGlassSystem(stage, {
      material: "thick",
      scene: sceneRef.current,
      motion: false,
      elasticity: 0.52,
      audio: { enabled: true, volume: 0.2, storage: null },
      artwork(context, { width, height, scene: currentScene }) {
        const image = currentScene === "ocean" ? backgroundImages.ocean : backgroundImages.aurora;
        if (drawBackgroundImage(context, image, width, height)) return;
        context.fillStyle = currentScene === "ocean" ? "#294e68" : "#d7e7e6";
        context.fillRect(0, 0, width, height);
      },
    });
    const redrawBackground = () => system.setScene(sceneRef.current);
    const pendingImages = [backgroundImages.aurora, backgroundImages.ocean]
      .filter(image => !image.complete);
    pendingImages.forEach(image => image.addEventListener("load", redrawBackground, { once: true }));
    const jelly = system.jelly as unknown as RuntimeJelly;
    const reflow = jelly.reflow.bind(jelly);
    jelly.reflow = () => {
      jelly.bodies.forEach((_body, element) => {
        if (!element.isConnected) jelly.bodies.delete(element);
      });
      reflow();
    };
    systemRef.current = system;
    system.mount(stage);
    return () => {
      systemRef.current = null;
      pendingImages.forEach(image => image.removeEventListener("load", redrawBackground));
      system.destroy();
    };
  }, []);

  useEffect(() => {
    const system = systemRef.current;
    if (!system) return;
    system.setScene(scene);
    system.renderer?.invalidate(500, true);
  }, [scene]);

  useEffect(() => {
    let settleTimer = 0;
    let firstFrame = 0;
    let secondFrame = 0;
    const handleResize = () => {
      window.clearTimeout(settleTimer);
      cancelAnimationFrame(firstFrame);
      cancelAnimationFrame(secondFrame);
      stageRef.current?.classList.add("vg-resizing");
      settleTimer = window.setTimeout(() => {
        systemRef.current?.renderer?.invalidate(0, true);
        firstFrame = requestAnimationFrame(() => {
          secondFrame = requestAnimationFrame(() => {
            stageRef.current?.classList.remove("vg-resizing");
          });
        });
      }, 120);
    };
    window.addEventListener("resize", handleResize, { passive: true });
    return () => {
      window.removeEventListener("resize", handleResize);
      window.clearTimeout(settleTimer);
      cancelAnimationFrame(firstFrame);
      cancelAnimationFrame(secondFrame);
      stageRef.current?.classList.remove("vg-resizing");
    };
  }, []);

  return (
    <LensRuntimeContext.Provider value={runtime}>
      <div
        ref={stageRef}
        className={cn("vg-stage lg-stage", className)}
        data-vg-app
        style={{
          backgroundImage: `url("${scene === "ocean" ? oceanBackground : auroraBackground}")`,
        }}
      >
        {children}
      </div>
    </LensRuntimeContext.Provider>
  );
}

export function LensPanel({
  label,
  className,
  children,
}: {
  label: string;
  className?: string;
  children: ReactNode;
}) {
  const runtime = useLensRuntime();
  const hostRef = useRef<HTMLDivElement>(null);
  const [surface, setSurface] = useState<HTMLElement | null>(null);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const handle = createComponent("panel", { label, motion: "none" });
    assignClass(handle.element, className);
    handle.element.classList.add("vg-lens-panel");
    host.replaceChildren(handle.element);
    setSurface(handle.element);
    mountWithRuntime(runtime, handle);
    return () => {
      setSurface(null);
      handle.destroy();
      handle.element.remove();
    };
  }, [className, label, runtime]);

  return (
    <div ref={hostRef} className="vg-component-slot">
      {surface ? createPortal(children, surface) : null}
    </div>
  );
}

export function LensActionButton({
  label,
  ariaLabel,
  variant = "default",
  className,
  disabled = false,
  sound,
  onPress,
}: {
  label: string;
  ariaLabel?: string;
  variant?: "default" | "success";
  className?: string;
  disabled?: boolean;
  sound?: string;
  onPress: () => void;
}) {
  const runtime = useLensRuntime();
  const hostRef = useRef<HTMLSpanElement>(null);
  const handleRef = useRef<ButtonHandle | null>(null);
  const actionRef = useRef(onPress);

  useEffect(() => {
    actionRef.current = onPress;
  }, [onPress]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const handle = createComponent("button", {
      label,
      variant,
      disabled,
      sound,
      onPress: () => actionRef.current(),
    });
    if (ariaLabel) handle.element.setAttribute("aria-label", ariaLabel);
    assignClass(handle.element, className);
    handle.element.classList.add("vg-lens-button");
    handleRef.current = handle;
    host.replaceChildren(handle.element);
    mountWithRuntime(runtime, handle);
    return () => {
      handleRef.current = null;
      handle.destroy();
      handle.element.remove();
    };
  }, [ariaLabel, className, disabled, label, runtime, sound, variant]);

  useEffect(() => {
    handleRef.current?.setDisabled(disabled);
  }, [disabled]);

  return <span ref={hostRef} className="vg-component-slot" />;
}

export function LensChoiceAction({
  label,
  selected,
  disabled = false,
  className,
  onPress,
}: {
  label: string;
  selected: boolean;
  disabled?: boolean;
  className?: string;
  onPress: () => void;
}) {
  const runtime = useLensRuntime();
  const hostRef = useRef<HTMLSpanElement>(null);
  const handleRef = useRef<ValueHandle<string> | null>(null);
  const actionRef = useRef(onPress);
  const selectedRef = useRef(selected);
  selectedRef.current = selected;

  useEffect(() => {
    actionRef.current = onPress;
  }, [onPress]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const handle = createComponent("choices", {
      label: "切换账号",
      options: [
        { value: "idle", label: "未选择" },
        { value: "switch", label: selectedRef.current ? "在线" : "切换" },
      ],
      value: selectedRef.current ? "switch" : "idle",
      selectionColor: "green",
      onChange: value => {
        if (value === "switch") actionRef.current();
      },
    });
    assignClass(handle.element, className);
    handle.element.classList.add("vg-account-choice");
    const idle = handle.element.querySelector<HTMLButtonElement>('button[data-choice="idle"]');
    if (idle) {
      idle.hidden = true;
      idle.tabIndex = -1;
      idle.setAttribute("aria-hidden", "true");
    }
    handleRef.current = handle;
    host.replaceChildren(handle.element);
    mountWithRuntime(runtime, handle);
    return () => {
      handleRef.current = null;
      handle.destroy();
      handle.element.remove();
    };
  }, [className, runtime]);

  useEffect(() => {
    const handle = handleRef.current;
    if (!handle) return;
    const button = handle.element.querySelector<HTMLButtonElement>('button[data-choice="switch"]');
    const isSelected = handle.value === "switch";
    const becameSelected = selected && !isSelected;

    if (selected && !isSelected) {
      handle.setValue("switch");
    } else if (!selected && isSelected) {
      handle.setValue("idle");
    }

    if (button) {
      button.disabled = disabled;
      button.setAttribute("aria-label", label);
      const text = button.querySelector("span");
      if (text) animateTextChange(text, label);
    }

    if (becameSelected && button && !matchMedia("(prefers-reduced-motion: reduce)").matches) {
      requestAnimationFrame(() => {
        const beam = button.querySelector<HTMLElement>(".lg-optic-beam");
        if (!beam || !button.isConnected) return;
        beam.getAnimations().forEach(animation => animation.cancel());
        beam.animate(
          [
            { transform: "rotate(-100deg)", opacity: 0 },
            { opacity: 0.92, offset: 0.18 },
            { transform: "rotate(260deg)", opacity: 0 },
          ],
          { duration: 980, easing: "cubic-bezier(0.16, 1, 0.3, 1)" },
        );
      });
    }
  }, [disabled, label, selected]);

  return <span ref={hostRef} className="vg-component-slot" />;
}

export function LensIconButton({
  label,
  icon,
  className,
  disabled = false,
  pressed,
  decorative = false,
  onChange,
  onPress,
}: {
  label: string;
  icon: OpticalIcon;
  className?: string;
  disabled?: boolean;
  pressed?: boolean;
  decorative?: boolean;
  onChange?: (pressed: boolean) => void;
  onPress?: () => void;
}) {
  const runtime = useLensRuntime();
  const hostRef = useRef<HTMLSpanElement>(null);
  const handleRef = useRef<ToggleHandle | null>(null);
  const actionRef = useRef(onPress);
  const changeRef = useRef(onChange);

  useEffect(() => {
    actionRef.current = onPress;
    changeRef.current = onChange;
  }, [onChange, onPress]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const handle = createComponent("icon-button", {
      label,
      icon,
      disabled,
      pressed,
      onChange: value => changeRef.current?.(value),
      onPress: () => actionRef.current?.(),
    });
    assignClass(handle.element, className);
    handle.element.classList.add("vg-lens-icon-button");
    if (decorative) {
      handle.element.classList.add("is-decorative");
      handle.element.tabIndex = -1;
      handle.element.setAttribute("aria-hidden", "true");
    }
    handleRef.current = handle;
    host.replaceChildren(handle.element);
    mountWithRuntime(runtime, handle);
    return () => {
      handleRef.current = null;
      handle.destroy();
      handle.element.remove();
    };
  }, [decorative, disabled, icon, label, pressed, runtime]);

  useEffect(() => {
    handleRef.current?.setDisabled(disabled);
  }, [disabled]);

  useEffect(() => {
    const handle = handleRef.current;
    if (typeof pressed === "boolean" && handle?.value !== pressed) handle?.setValue(pressed);
  }, [pressed]);

  return <span ref={hostRef} className="vg-component-slot" />;
}

export function LensSwitch({
  label,
  checked,
  disabled = false,
  className,
  onChange,
}: {
  label: string;
  checked: boolean;
  disabled?: boolean;
  className?: string;
  onChange: (checked: boolean) => void;
}) {
  const runtime = useLensRuntime();
  const hostRef = useRef<HTMLSpanElement>(null);
  const handleRef = useRef<ValueHandle<boolean> | null>(null);
  const changeRef = useRef(onChange);

  useEffect(() => {
    changeRef.current = onChange;
  }, [onChange]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const handle = createComponent("switch", {
      label,
      checked,
      onChange: value => changeRef.current(value),
    });
    assignClass(handle.element, className);
    handle.element.classList.add("vg-lens-switch");
    handleRef.current = handle;
    host.replaceChildren(handle.element);
    mountWithRuntime(runtime, handle);
    return () => {
      handleRef.current = null;
      handle.destroy();
      handle.element.remove();
    };
  }, [handleRef, label, runtime]);

  useEffect(() => {
    handleRef.current?.setValue(checked);
    if (handleRef.current) (handleRef.current.element as HTMLButtonElement).disabled = disabled;
  }, [checked, disabled]);

  return <span ref={hostRef} className="vg-component-slot" />;
}

export function LensToggleTile({
  label,
  icon,
  checked,
  onLabel,
  offLabel,
  className,
  disabled = false,
  onChange,
}: {
  label: string;
  icon: OpticalIcon;
  checked: boolean;
  onLabel: string;
  offLabel: string;
  className?: string;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
}) {
  const runtime = useLensRuntime();
  const hostRef = useRef<HTMLSpanElement>(null);
  const handleRef = useRef<ToggleHandle | null>(null);
  const changeRef = useRef(onChange);
  const checkedRef = useRef(checked);

  useEffect(() => {
    changeRef.current = onChange;
  }, [onChange]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const handle = createComponent("toggle-tile", {
      label,
      icon,
      checked,
      onLabel,
      offLabel,
      disabled,
      onChange: value => changeRef.current(value),
    });
    assignClass(handle.element, className);
    handle.element.classList.add("vg-lens-toggle-tile");
    handleRef.current = handle;
    checkedRef.current = checked;
    host.replaceChildren(handle.element);
    mountWithRuntime(runtime, handle);
    return () => {
      handleRef.current = null;
      handle.destroy();
      handle.element.remove();
    };
  }, [icon, label, runtime]);

  useEffect(() => {
    const handle = handleRef.current;
    if (!handle) return;
    const status = handle.element.querySelector<HTMLElement>(".lg-tile-status");
    const previousStatus = status?.textContent ?? "";
    if (checkedRef.current !== checked) {
      handle.setValue(checked);
      checkedRef.current = checked;
      if (status) status.textContent = previousStatus;
    }
    handle.setDisabled(disabled);
    animateTextChange(status, checked ? onLabel : offLabel);
  }, [checked, disabled, offLabel, onLabel]);

  return <span ref={hostRef} className="vg-component-slot" />;
}

export function LensChoices({
  label,
  value,
  options,
  className,
  onChange,
}: {
  label: string;
  value: string;
  options: Array<{ value: string; label: string }>;
  className?: string;
  onChange: (value: string) => void;
}) {
  const runtime = useLensRuntime();
  const hostRef = useRef<HTMLSpanElement>(null);
  const handleRef = useRef<ValueHandle<string> | null>(null);
  const changeRef = useRef(onChange);
  const valueRef = useRef(value);
  const optionsKey = options.map(option => `${option.value}:${option.label}`).join("|");

  useEffect(() => {
    changeRef.current = onChange;
    valueRef.current = value;
  }, [onChange, value]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const handle = createComponent("choices", {
      label,
      options: options.map(option => ({ ...option })),
      value: valueRef.current,
      selectionColor: "green",
      onChange: next => changeRef.current(next),
    });
    assignClass(handle.element, className);
    handle.element.classList.add("vg-lens-choices");
    handleRef.current = handle;
    host.replaceChildren(handle.element);
    mountWithRuntime(runtime, handle);
    return () => {
      handleRef.current = null;
      handle.destroy();
      handle.element.remove();
    };
  }, [label, optionsKey, runtime]);

  useEffect(() => {
    handleRef.current?.setValue(value);
  }, [value]);

  return <span ref={hostRef} className="vg-component-slot" />;
}

export function LensTabs({
  label,
  value,
  options,
  className,
  onChange,
}: {
  label: string;
  value: string;
  options: Array<{ value: string; label: string }>;
  className?: string;
  onChange: (value: string) => void;
}) {
  const runtime = useLensRuntime();
  const hostRef = useRef<HTMLSpanElement>(null);
  const handleRef = useRef<ValueHandle<string> | null>(null);
  const changeRef = useRef(onChange);
  const valueRef = useRef(value);
  const optionsKey = options.map(option => `${option.value}:${option.label}`).join("|");

  useEffect(() => {
    changeRef.current = onChange;
    valueRef.current = value;
  }, [onChange, value]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const handle = createComponent("tabs", {
      label,
      options: options.map(option => ({ ...option, content: "" })),
      value: valueRef.current,
      onChange: next => changeRef.current(next),
    });
    assignClass(handle.element, className);
    handle.element.classList.add("vg-lens-tabs");
    handleRef.current = handle;
    host.replaceChildren(handle.element);
    mountWithRuntime(runtime, handle);
    return () => {
      handleRef.current = null;
      handle.destroy();
      handle.element.remove();
    };
  }, [label, optionsKey, runtime]);

  useEffect(() => {
    handleRef.current?.setValue(value);
  }, [value]);

  return <span ref={hostRef} className="vg-component-slot" />;
}

export function LensSearchField({
  label,
  value,
  placeholder,
  type = "text",
  autoComplete,
  className,
  onChange,
}: {
  label: string;
  value: string;
  placeholder?: string;
  type?: "text" | "search" | "password";
  autoComplete?: string;
  className?: string;
  onChange: (value: string) => void;
}) {
  const runtime = useLensRuntime();
  const hostRef = useRef<HTMLSpanElement>(null);
  const handleRef = useRef<ComponentHandle & { setValue: (value: string) => void } | null>(null);
  const changeRef = useRef(onChange);

  useEffect(() => {
    changeRef.current = onChange;
  }, [onChange]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const handle = createComponent("search", {
      label,
      placeholder,
      onChange: next => changeRef.current(next),
    });
    const input = handle.element.querySelector("input");
    if (input) {
      input.type = type;
      if (autoComplete) input.setAttribute("autocomplete", autoComplete);
      input.spellcheck = false;
    }
    assignClass(handle.element, className);
    handle.element.classList.add("vg-lens-search");
    handleRef.current = handle;
    host.replaceChildren(handle.element);
    mountWithRuntime(runtime, handle);
    return () => {
      handleRef.current = null;
      handle.destroy();
      handle.element.remove();
    };
  }, [autoComplete, label, placeholder, runtime, type]);

  useEffect(() => {
    handleRef.current?.setValue(value);
    const input = handleRef.current?.element.querySelector("input");
    if (input) input.type = type;
  }, [type, value]);

  return <span ref={hostRef} className="vg-component-slot" />;
}

export function LensProgress({
  label,
  statusLabel = label,
  value,
  animate = true,
  className,
}: {
  label: string;
  statusLabel?: string;
  value: number;
  animate?: boolean;
  className?: string;
}) {
  const runtime = useLensRuntime();
  const hostRef = useRef<HTMLSpanElement>(null);
  const handleRef = useRef<OpticalProgressHandle | null>(null);
  const animateRef = useRef(animate);
  const initialStatus = useRef(statusLabel);

  useEffect(() => {
    animateRef.current = animate;
  }, [animate]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const handle = createComponent("progress", {
      label: initialStatus.current,
      value,
      completionColor: "green",
      completeLabel: "已完成",
      wave: true,
      motion: "system",
    });
    assignClass(handle.element, className);
    handle.element.classList.add("vg-lens-progress");
    handle.element
      .querySelector<HTMLElement>(".lg-progress-track")
      ?.setAttribute("aria-label", label);
    handleRef.current = handle;
    host.replaceChildren(handle.element);
    mountWithRuntime(runtime, handle);
    return () => {
      handleRef.current = null;
      handle.destroy();
      handle.element.remove();
    };
  }, [runtime]);

  useEffect(() => {
    const handle = handleRef.current;
    if (!handle) return;
    handle.element
      .querySelector<HTMLElement>(".lg-progress-track")
      ?.setAttribute("aria-label", label);
    const heading = handle.element.querySelector<HTMLElement>(".lg-progress-heading > span");
    animateTextChange(heading, statusLabel);
  }, [label, statusLabel]);

  useEffect(() => {
    handleRef.current?.setValue(value, { animate: animateRef.current });
  }, [value]);

  return <span ref={hostRef} className="vg-component-slot" />;
}

export type LensMenuOption = DropOption & { label: string };

export function LensMenu({
  label,
  options,
  className,
  triggerClassName,
  open,
  onOpenChange,
  onAction,
}: {
  label: string;
  options: LensMenuOption[];
  className?: string;
  triggerClassName?: string;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  onAction: (value: string) => void;
}) {
  const runtime = useLensRuntime();
  const hostRef = useRef<HTMLSpanElement>(null);
  const handleRef = useRef<MenuHandle | null>(null);
  const actionRef = useRef(onAction);
  const openChangeRef = useRef(onOpenChange);
  const optionsKey = options.map(option => `${option.value}:${option.label}:${Boolean(option.disabled)}:${option.icon ?? ""}`).join("|");

  useEffect(() => {
    actionRef.current = onAction;
    openChangeRef.current = onOpenChange;
  }, [onAction, onOpenChange]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const handle = createComponent("menu", {
      label,
      options: options.map(option => ({ ...option })),
      onAction: value => actionRef.current(value),
    });
    assignClass(handle.element, className);
    handle.element.classList.add("vg-lens-menu");
    const trigger = handle.element.querySelector<HTMLButtonElement>("button");
    trigger?.classList.add("vg-menu-trigger");
    if (trigger) {
      if (triggerClassName) trigger.classList.add(...triggerClassName.split(/\s+/).filter(Boolean));
      trigger.setAttribute("aria-label", label);
      [...trigger.childNodes].forEach(node => {
        if (node.nodeType === Node.TEXT_NODE) node.remove();
      });
    }
    const panel = handle.element.querySelector<HTMLElement>(".lg-menu-popover");
    let wasOpen = Boolean(panel && !panel.hidden);
    const observer = new MutationObserver(() => {
      if (!panel) return;
      const isOpen = !panel.hidden;
      if (isOpen === wasOpen) return;
      wasOpen = isOpen;
      openChangeRef.current?.(isOpen);
    });
    if (panel) observer.observe(panel, { attributes: true, attributeFilter: ["hidden"] });
    handleRef.current = handle;
    host.replaceChildren(handle.element);
    mountWithRuntime(runtime, handle);
    return () => {
      handleRef.current = null;
      observer.disconnect();
      handle.destroy();
      handle.element.remove();
    };
  }, [label, optionsKey, runtime, triggerClassName]);

  useEffect(() => {
    if (typeof open !== "boolean") return;
    if (open) handleRef.current?.open();
    else handleRef.current?.close();
  }, [open]);

  return <span ref={hostRef} className="vg-component-slot" />;
}

export function LensLens({
  label,
  active,
  className,
}: {
  label: string;
  active: boolean;
  className?: string;
}) {
  const runtime = useLensRuntime();
  const hostRef = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const handle = createComponent("lens", { label, caption: "" });
    assignClass(handle.element, className);
    handle.element.classList.add("vg-lens-orb");
    handle.element.dataset.active = String(active);
    host.replaceChildren(handle.element);
    mountWithRuntime(runtime, handle);
    return () => {
      handle.destroy();
      handle.element.remove();
    };
  }, [label, runtime]);

  useEffect(() => {
    const element = hostRef.current?.querySelector<HTMLElement>(".vg-lens-orb");
    if (element) element.dataset.active = String(active);
  }, [active]);

  return <span ref={hostRef} className="vg-component-slot" />;
}

export function LensDialog({
  open,
  onClose,
  title,
  description,
  children,
  footer,
  width = 440,
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  description?: string;
  children: ReactNode;
  footer?: ReactNode;
  width?: number;
}) {
  useEffect(() => {
    if (!open) return;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [onClose, open]);

  if (!open) return null;

  return (
    <div
      className="vg-dialog-layer"
      onPointerDown={event => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <LensPanel label={title} className="vg-dialog-panel fallback">
        <div className="vg-dialog-inner" style={{ width }}>
          <header className="vg-dialog-heading">
            <div>
              <h2>{title}</h2>
              {description ? <p>{description}</p> : null}
            </div>
            <LensActionButton label="关闭" className="vg-dialog-close" onPress={onClose} />
          </header>
          <div className="vg-dialog-body">{children}</div>
          {footer ? <footer className="vg-dialog-footer">{footer}</footer> : null}
        </div>
      </LensPanel>
    </div>
  );
}

type NotificationTone = "success" | "error" | "info";
type ToastEntry = { id: number; tone: NotificationTone; message: string; closing: boolean };
type ToastListener = (entry: ToastEntry) => void;

const TOAST_LIFETIME_MS = 1300;
const TOAST_EXIT_MS = 200;
const toastListeners = new Set<ToastListener>();
let nextToastId = 0;

function emitToast(tone: NotificationTone, message: string) {
  const entry = { id: ++nextToastId, tone, message, closing: false };
  toastListeners.forEach(listener => listener(entry));
}

export const lensToast = {
  success: (message: string) => emitToast("success", message),
  error: (message: string) => emitToast("error", message),
  info: (message: string) => emitToast("info", message),
};

function LensNotificationItem({
  tone,
  message,
  closing,
  onDismiss,
}: {
  tone: NotificationTone;
  message: string;
  closing: boolean;
  onDismiss: () => void;
}) {
  const runtime = useLensRuntime();
  const hostRef = useRef<HTMLSpanElement>(null);
  const dismissRef = useRef(onDismiss);

  useEffect(() => {
    dismissRef.current = onDismiss;
  }, [onDismiss]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    let dismissTimer = 0;
    const handle = createComponent("notification", {
      title: tone === "success" ? message : tone === "error" ? "操作未完成" : "提示",
      description: tone === "success" ? "" : message,
      dismissLabel: "关闭通知",
      onDismiss: () => {
        handle.element.hidden = false;
        handle.element.classList.add("is-closing");
        dismissTimer = window.setTimeout(() => dismissRef.current(), TOAST_EXIT_MS);
      },
    });
    handle.element.classList.add("vg-toast");
    handle.element.dataset.tone = tone;
    host.replaceChildren(handle.element);
    mountWithRuntime(runtime, handle);
    return () => {
      window.clearTimeout(dismissTimer);
      handle.destroy();
      handle.element.remove();
    };
  }, [message, runtime, tone]);

  useEffect(() => {
    const element = hostRef.current?.querySelector<HTMLElement>(".lg-notification");
    element?.classList.toggle("is-closing", closing);
  }, [closing]);

  return <span ref={hostRef} className="vg-component-slot" />;
}

export function LensNotificationHost() {
  const [entries, setEntries] = useState<ToastEntry[]>([]);
  const timers = useRef(new Map<number, ReturnType<typeof setTimeout>[]>());

  function clearTimers(id: number) {
    timers.current.get(id)?.forEach(clearTimeout);
    timers.current.delete(id);
  }

  function dismiss(id: number) {
    clearTimers(id);
    setEntries(current => current.filter(entry => entry.id !== id));
  }

  useEffect(() => {
    const listener: ToastListener = entry => {
      setEntries(current => [...current.filter(item => item.message !== entry.message), { ...entry, closing: false }].slice(-3));
      timers.current.set(entry.id, [
        setTimeout(() => {
          setEntries(current => current.map(item => (
            item.id === entry.id ? { ...item, closing: true } : item
          )));
        }, TOAST_LIFETIME_MS - TOAST_EXIT_MS),
        setTimeout(() => dismiss(entry.id), TOAST_LIFETIME_MS),
      ]);
    };
    toastListeners.add(listener);
    return () => {
      toastListeners.delete(listener);
      timers.current.forEach(timerList => timerList.forEach(clearTimeout));
      timers.current.clear();
    };
  }, []);

  return (
    <div className="vg-toast-host" aria-live="polite">
      {entries.map(entry => (
        <LensNotificationItem
          key={entry.id}
          tone={entry.tone}
          message={entry.message}
          closing={entry.closing}
          onDismiss={() => dismiss(entry.id)}
        />
      ))}
    </div>
  );
}
