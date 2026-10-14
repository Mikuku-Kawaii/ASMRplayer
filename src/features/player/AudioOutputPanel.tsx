import { useEffect, useId, useRef, useState } from "react";
import { Headphones, RefreshCw } from "lucide-react";
import { Select } from "../../components/Select";
import { AppError, AudioOutputStateDto, PlaybackSnapshot, callCommand, normalizeAppError } from "../../lib/api";

interface Props {
  snapshot: PlaybackSnapshot;
  onSnapshotChange: (snapshot: PlaybackSnapshot) => void;
  onError: (error: AppError) => void;
}

export function AudioOutputPanel({ snapshot, onSnapshotChange, onError }: Props) {
  const modeDescriptionId = useId();
  const [output, setOutput] = useState<AudioOutputStateDto | null>(null);
  const [busy, setBusy] = useState(false);
  const [loadError, setLoadError] = useState("");
  const inFlight = useRef(false);
  const request = useRef(0);

  useEffect(() => {
    const id = ++request.current;
    setBusy(true);
    inFlight.current = true;
    void callCommand<AudioOutputStateDto>("get_audio_output").then(next => {
      if (request.current === id) { setOutput(next); setLoadError(""); }
    }).catch(error => {
      if (request.current === id) setLoadError(normalizeAppError(error).message);
    }).finally(() => {
      if (request.current === id) { setBusy(false); inFlight.current = false; }
    });
    return () => { request.current = id + 1; };
  }, []);

  async function update(action: "refresh" | "device" | "exclusive", deviceId?: string) {
    if (inFlight.current) return;
    inFlight.current = true;
    const id = ++request.current;
    setBusy(true);
    try {
      if (action === "exclusive") {
        const next = await callCommand<PlaybackSnapshot>("set_audio_exclusive", { exclusive: !snapshot.audioExclusive });
        onSnapshotChange(next);
      } else {
        const next = await callCommand<AudioOutputStateDto>(action === "device" ? "set_audio_device" : "get_audio_output",
          action === "device" ? { deviceId } : undefined);
        if (request.current === id) { setOutput(next); setLoadError(""); }
      }
    } catch (error) {
      if (request.current === id) {
        const normalized = normalizeAppError(error);
        if (action === "refresh") setLoadError(normalized.message);
        else onError(normalized);
      }
    } finally {
      if (request.current === id) { setBusy(false); inFlight.current = false; }
    }
  }

  const devices = output?.devices ?? [];
  const unavailable = !!output && !devices.some(device => device.id === output.selectedDevice);
  return <div className="drawer-body audio-output-panel" aria-busy={busy}>
    <section className="drawer-section">
      <button type="button" role="switch" aria-label="WASAPI 独占模式" aria-checked={Boolean(snapshot.audioExclusive)}
        aria-describedby={modeDescriptionId} className="audio-output-mode"
        title={snapshot.audioExclusive ? "关闭独占，切回共享模式" : "开启独占模式"}
        disabled={busy} onClick={() => void update("exclusive")}>
        <span className="audio-output-mode-heading">
          <span className="audio-output-mode-title"><Headphones size={18} aria-hidden="true" />输出模式</span>
          <span className="audio-output-mode-state">{snapshot.audioExclusive ? "独占已开启" : "独占已关闭"}</span>
        </span>
        <span className="audio-output-mode-name">{snapshot.audioExclusive ? "WASAPI 独占模式" : "WASAPI 共享模式"}</span>
        <span id={modeDescriptionId} className="audio-output-mode-description">
          {snapshot.audioExclusive
            ? "播放器正在独占所选耳机或音箱，其他应用可能暂时无法通过它发声。如果还想听到浏览器、游戏或系统提示音，点击这里切回共享模式。"
            : "当前与其他应用共用所选耳机或音箱，可以同时听到播放器、浏览器和系统提示音。想让播放器单独使用这个设备时，点击这里开启独占；开启后，其他应用可能暂时无法通过它发声。"}
        </span>
      </button>
    </section>
    <section className="drawer-section audio-output-device">
      <div className="audio-output-device-heading"><label id="audio-device-label" htmlFor="audio-output-device">输出设备</label>
        <button type="button" className="btn-icon-sm" aria-label="刷新输出设备" title="刷新输出设备" disabled={busy}
          onClick={() => void update("refresh")}><RefreshCw size={15} /></button></div>
      <Select id="audio-output-device" aria-labelledby="audio-device-label" value={output?.selectedDevice ?? ""}
        placeholder={busy && !output ? "正在读取设备…" : "请选择输出设备"} disabled={busy || !output}
        options={[
          ...(unavailable ? [{ value: output!.selectedDevice, label: "已保存的设备（当前不可用）", disabled: true }] : []),
          ...devices.map(device => ({ value: device.id, label: device.name })),
        ]} onValueChange={deviceId => void update("device", deviceId)} />
      {unavailable && <p className="text-muted">设备已断开，可重新连接或选择系统默认设备。</p>}
      {loadError && <p role="alert" className="audio-output-error">{loadError}</p>}
    </section>
  </div>;
}
