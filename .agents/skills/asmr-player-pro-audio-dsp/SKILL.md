---
name: asmr-player-pro-audio-dsp
description: "Professional audiophile player engine (foobar2000 / Neutron Music Player grade) and ASMR-specialized DSP / EQ / Peak Limiter chain specification. MUST USE whenever designing, implementing, refactoring, or testing audio playback fidelity, DSP filter graphs, Parametric EQ (PEQ), ASMR filter tuning (40-60Hz Sub-Bass HPF, 5-8kHz De-Esser/Sibilance notch, 10-14kHz Whisper Air shelf), AutoEQ preset import/parsing, Anti-Ear-Blast Lookahead Soft Peak Limiter, audio stream telemetry, or DSP/EQ UI controls."
---

# ASMR Player · Pro Audio Engine & ASMR DSP / EQ Chain Skill

## When To Load This Skill

Load this skill whenever working on audio processing, player precision, DSP/EQ pipelines, or audiophile control surfaces, including:

- Designing or modifying the DSP/EQ signal chain in `MpvController` (`src-tauri/src/player/mod.rs`) or `PlaybackService` (`src-tauri/src/services/playback.rs`).
- Implementing or tuning the **ASMR 3-Stage Specialty Filters**:
  1. **Sub-Bass Rumble Cut** ($40\text{ Hz}\text{–}60\text{ Hz}$, $12\text{ dB/oct}$ High-Pass Filter).
  2. **Sibilance & Harshness Suppressor / De-Esser** ($5\text{ kHz}\text{–}8\text{ kHz}$ narrow-$Q$ Peaking / Notch / Dynamic De-Esser).
  3. **Whisper & Breath Air Enhancer** ($10\text{ kHz}\text{–}14\text{ kHz}$ High-Shelf Filter).
- Implementing **AutoEQ** preset parsing, validation, and management (EqualizerAPO `ParametricEQ.txt`, `Preamp`, `PK`/`LSC`/`HSC` biquad filters).
- Implementing or tuning the **Anti-Ear-Blast Protection** (**Lookahead Soft Peak Limiter**, $3\text{–}5\text{ ms}$ lookahead, $-1.0\text{ dBFS}$ ceiling).
- Adding foobar2000 / Neutron-style technical stream telemetry (Codec, Sample Rate, Bit Depth, Bitrate, Channel Layout, Active DSP chain status) or frequency-response visualization.
- Building the Frontend DSP / EQ Drawer, Preset Manager, or Audio Status Bar (in conjunction with `asmr-player-m3-ui-standards`).

---

## Core Philosophy: foobar2000 / Neutron Precision Meets ASMR Psychoacoustics

1. **Bit-Perfect Transparency When Bypassed, 64-Bit/Float Precision When Active (直通零染色，开启高精度)**
   - 借鉴 **foobar2000** 与 **Neutron Music Player** 的核心准则：当全部 DSP 关闭（Bypass / 直通模式）时，音频流必须保持 100% 原始双声道直通，绝不注入任何隐式重采样、自动增益或动态压缩。
   - 当任一 DSP 模块启用时，内部滤波与动态处理全程运行于浮点域（`fltp` / `f32` / `f64`），并在滤波前通过 **Preamp（前置衰减）** 预留净空（Headroom），彻底杜绝滤波器级间削波（Inter-stage Clipping）。

2. **Binaural Phase & Imaging Sanctity (双耳时间差/相位差神圣不可侵犯)**
   - ASMR 录音（如 Neumann KU100 人头麦、3Dio 双耳麦）极度依赖左右耳微秒级的到达时间差（ITD）与声压差（ILD）来构建贴耳 3D 空间感。
   - **铁律 1**：所有 EQ 与滤波器必须在左右声道使用**完全一致的滤波器系数与相位响应**，严禁引入非对称相移或声道串扰（除非用户显式调节左右平衡）。
   - **铁律 2**：防炸耳限制器（Peak Limiter）的增益衰减必须**双声道强制耦合（Stereo-Linked Gain Reduction）**——左耳出现突发瞬态峰值时，左右声道必须同步等量衰减，绝不能导致声像在衰减瞬间向另一侧漂移。

3. **Zero Upward Pumping on Quiet Whispers (严禁抬升微弱底噪)**
   - ASMR 听众追求极致的微弱细节与安静背景。防炸耳限制器**只能向下压制超过阈值的突发巨响（Downward Peak Limiting Only）**，严禁开启自动增益补偿（Auto Make-up Gain / Automatic Leveling），否则会在安静段把麦克风底噪（Self-noise）和呼吸空白疯狂放大。

4. **Glitch-Free Real-Time Control (热切换不断流、不丢进度)**
   - 调整 EQ 频段、切换 AutoEQ 预设、开关防炸耳限制器时，必须通过 `mpv` JSON IPC 的 `["set_property", "af", ...]` 实时热更新滤镜图，**绝不能重启 `mpv` 进程或导致播放位置回跳**。

---

## Part 1: Canonical DSP Signal Chain Topology (标准串联信号链拓扑)

在音频工程中，**模块的串联顺序直接决定音质成败**（例如：高通滤波必须在限制器之前，否则次声波麦克风碰撞会误触发限制器产生抽吸效应；防炸耳限制器必须位于整条链路的最后一环，以兜底前面所有 EQ 增益带来的潜在削波）。

本播放器固定采用以下 **8 级确定性串联拓扑**：

```text
[Stage 0: Source Decode] ── 原始双声道解码 (FLAC/WAV/MP3/M4A/Opus -> Float Planar)
       │
       ▼
[Stage 1: Preamp & Headroom Compensation (前置增益 / AutoEQ Preamp)]
       │  范围: -15.0 dB ~ +6.0 dB (自动读取 AutoEQ Preamp 或抵消正向 EQ 峰值)
       ▼
[Stage 2: ASMR Sub-Bass Rumble Cut (超低频截断高通滤波器 HPF)]
       │  频段: 40 Hz – 60 Hz (默认 50 Hz) | 斜率: 12 dB/oct (2阶 Butterworth, Q = 0.7071)
       │  作用: 滤除麦克风支架震动、手部触碰次声波轰鸣与空调低频底噪
       ▼
[Stage 3: AutoEQ Headphone Calibration PEQ (耳机频响校准参数均衡器)]
       │  结构: 1 ~ 10 段级联 Biquad (支持 PK 峰值 / LSC 低架 / HSC 高架)
       │  作用: 一键导入 AutoEQ 参数，将用户耳机频响校准至 Harman / 扩散场目标曲线
       ▼
[Stage 4: ASMR Sibilance & Harshness Suppressor (齿音与瞬态刺耳压制器)]
       │  频段: 5 kHz – 8 kHz (默认 6.5 kHz) | 窄带 Q: 2.5 – 8.0 (默认 4.0) | 衰减: 0 ~ -8.0 dB (默认 -3.5 dB)
       │  作用: 精准压制咀嚼音、舌音、掏耳竹签敲击在耳道共振区产生的尖锐刺耳能量
       ▼
[Stage 5: ASMR Whisper & Breath Air Enhancer (呼吸与贴耳空气感增强)]
       │  频段: 10 kHz – 14 kHz (默认 12 kHz) | 类型: High-Shelf (Q = 0.7071) | 增益: 0 ~ +3.0 dB (默认 +1.5 dB)
       │  作用: 柔顺托起耳边气音与呼吸泛音，增强贴耳酥麻感且不触发 6 kHz 齿音
       ▼
[Stage 6: L/R Channel Balance Trim (左右耳微调，可选)]
       │  范围: L -6.0 dB ~ R +6.0 dB (默认 0.0 dB 严格等幅，补偿人耳左右敏度差异或偏音录音)
       ▼
[Stage 7: Anti-Ear-Blast Lookahead Soft Peak Limiter (防炸耳前瞻软限制器)]
       │  前瞻 (Lookahead): 3 ms – 5 ms (默认 4.0 ms)
       │  天花板阈值 (Ceiling): -1.0 dBFS (可调 -0.5 ~ -12.0 dBFS，禁用自动增益补偿 level=0)
       │  作用: 透明拦截意外碰撞、笑声、爆音，保护高音量收听下的鼓膜安全
       ▼
[Stage 8: Hardware Output] ── Windows Audio Session (WASAPI)
```

---

## Part 2: ASMR Specialized Filter & EQ Tuning Specifications (滤波与 EQ 专项规范)

### 2.1 Stage 2 · 超低频截断（Sub-Bass Rumble Cut — High-Pass / Low-Cut）

- **声学机理**：绝大多数人声耳语、触发道具在 $60\text{ Hz}$ 以下没有任何有效基频或泛音，该频段充斥着手指触碰麦克风外壳的次声波轰鸣（Sub-bass rumble）、呼吸喷麦气流（Plosives）以及环境低频驻波。
- **滤波器数学与工程参数**：
  - **滤波器类型**：二阶巴特沃斯高通滤波器（2nd-Order Butterworth High-Pass Filter，即级联单节二阶 Biquad，`poles=2`）。
  - **滚降斜率（Slope）**：**$12\text{ dB/octave}$**（既能干净切除次声波轰鸣，又不会像 $24\text{–}48\text{ dB/oct}$ 陡峭滤波器那样在截止频率附近引入明显的群延迟振铃效应）。
  - **截止频率 ($f_c$)**：
    - 标准 ASMR 调节范围：**$40\text{ Hz} \text{ – } 60\text{ Hz}$**（专家模式可展开至 $20\text{ Hz} \text{ – } 100\text{ Hz}$）。
    - 默认推荐值：**$50\text{ Hz}$**。
  - **品质因数 ($Q$)**：固定 **$Q = \frac{\sqrt{2}}{2} \approx 0.7071$**（保证通带最大平坦度，在 $50\text{ Hz}$ 拐点处绝不产生谐振鼓包）。
- **`mpv` / FFmpeg `lavfi` 映射表达式**：
  ```text
  highpass=f=50:poles=2:width_type=q:width=0.7071
  ```

### 2.2 Stage 4 · 齿音压制（Sibilance Suppressor — Narrow-Q Peaking / Notch Filter）

- **声学机理**：口腔音（Mouth sounds）、咀嚼音、舌音、金属/竹制采耳工具敲击音，极易在 **$5\text{ kHz} \text{ – } 8\text{ kHz}$** 产生高能量窄带瞬态峰值。该频段恰好重合于人耳外耳道的第一共振峰，长时间聆听极易引发听觉疲劳与烦躁感。
- **滤波器数学与工程参数**：
  - **滤波器类型**：窄带峰值衰减滤波器（Narrow-$Q$ Peaking Attenuation Filter / Surgical Bell Notch），充当零相位失真的静态去齿音器（Static De-Esser），可选支持动态去齿音模式（Dynamic De-Esser）。
  - **中心频率 ($f_0$)**：
    - 调节范围：**$5000\text{ Hz} \text{ – } 8000\text{ Hz}$**（步进 $50\text{ Hz}$）。
    - 默认推荐值：**$6500\text{ Hz}$**（可针对女声齿音调至 $6.8\text{–}7.5\text{ kHz}$，针对口腔湿音/敲击音调至 $5.2\text{–}6.2\text{ kHz}$）。
  - **品质因数 ($Q$)**：
    - 调节范围：**$2.5 \text{ – } 8.0$**（高 $Q$ 窄带宽设计，确保只像手术刀一样挖掉尖锐齿音峰，不损伤 $2\text{–}4\text{ kHz}$ 的人声清晰度与 $10\text{ kHz}$ 以上的空气感）。
    - 默认推荐值：**$4.0$**。
  - **衰减增益 ($G$)**：
    - 调节范围：**$0.0\text{ dB} \text{ 至 } -8.0\text{ dB}$**（步进 $0.5\text{ dB}$，**严禁在此模块填入正增益**）。
    - 默认推荐值：**$-3.5\text{ dB}$**（轻度柔化：$-2.0\text{ dB}$；强力去齿音：$-5.5\text{ dB}$）。
- **`mpv` / FFmpeg `lavfi` 映射表达式**：
  ```text
  equalizer=f=6500:width_type=q:width=4.0:g=-3.5
  ```

### 2.3 Stage 5 · 空气感增强（Air Enhancer — High-Shelf Filter）

- **声学机理**：耳边轻语（Whispering）、柔和呼吸声、羽毛/毛刷拂过麦克风的细腻泛音集中在 **$10\text{ kHz} \text{ – } 14\text{ kHz}$** 频段。由于前面 Stage 4 已经压制了 $5\text{–}8\text{ kHz}$ 的刺耳区，此时在 $10\text{–}14\text{ kHz}$ 施加微量的搁架式提升，能够显著拉近声源距离、增强“贴着耳廓呼吸”的酥麻空气感，而不会带来毛刺感。
- **滤波器数学与工程参数**：
  - **滤波器类型**：二阶高频搁架滤波器（High-Shelf Filter, `HSC`）。
  - **转折频率 ($f_s$)**：
    - 调节范围：**$10000\text{ Hz} \text{ – } 14000\text{ Hz}$**（步进 $200\text{ Hz}$）。
    - 默认推荐值：**$12000\text{ Hz}$**。
  - **斜率品质因数 ($Q$)**：固定 **$Q = 0.7071$**（平滑无过冲搁架过渡）。
  - **提升增益 ($G$)**：
    - 调节范围：**$0.0\text{ dB} \text{ 至 } +3.0\text{ dB}$**（步进 $0.25\text{ dB}$）。
    - 默认推荐范围：**$+1.0\text{ dB} \text{ – } +2.0\text{ dB}$**（默认 **$+1.5\text{ dB}$**；若增益 $> 0\text{ dB}$，需在 Stage 1 自动微调补偿净空或由 Stage 7 防炸耳限制器兜底）。
- **`mpv` / FFmpeg `lavfi` 映射表达式**：
  ```text
  highshelf=f=12000:width_type=q:width=0.7071:g=1.5
  ```

---

## Part 3: AutoEQ Preset Import & Parametric EQ Specification (AutoEQ 兼容规范)

播放器必须原生支持一键导入或粘贴来自 [AutoEQ](https://github.com/jaakkopasanen/AutoEq) 仓库的标准参数均衡配置（`ParametricEQ.txt` / EqualizerAPO 格式）。

### 3.1 标准 AutoEQ 输入格式语法

```text
Preamp: -6.4 dB
Filter 1: ON LSC Fc 105 Hz Gain 6.6 dB Q 0.70
Filter 2: ON PK Fc 210 Hz Gain -1.8 dB Q 1.15
Filter 3: ON PK Fc 1450 Hz Gain -2.4 dB Q 1.80
Filter 4: ON PK Fc 3850 Hz Gain 4.1 dB Q 2.60
Filter 5: ON HSC Fc 10000 Hz Gain -3.2 dB Q 0.70
```

### 3.2 解析器与校验规则（Rust 端实现铁律）

1. **逐行容错解析**：
   - 忽略空行与 `#` 开头的注释行。
   - 匹配 `Preamp:\s*([+-]?\d+(?:\.\d+)?)\s*dB`（大小写不敏感），提取 `preamp_db`。
   - 匹配 `Filter\s+\d+:\s*(ON|OFF)\s+(PK|PEQ|LSC|LS|HSC|HS)\s+Fc\s+(\d+(?:\.\d+)?)\s*Hz\s+Gain\s+([+-]?\d+(?:\.\d+)?)\s*dB\s+Q\s+(\d+(?:\.\d+)?)`。
   - 若某行标为 `OFF`，保留在预设结构中但生成 `lavfi` 滤镜链时跳过。
2. **安全边界硬校验（防 Biquad 发散自激）**：
   - 频率 $f_c \in [20.0\text{ Hz}, 20000.0\text{ Hz}]$
   - 增益 $\text{Gain} \in [-24.0\text{ dB}, +24.0\text{ dB}]$
   - 品质因数 $Q \in [0.1, 20.0]$
   - 前置增益 $\text{Preamp} \in [-24.0\text{ dB}, +12.0\text{ dB}]$
   - 超出范围或格式完全无效时，返回 `ErrorCode::InvalidInput` 并提示具体行号，**绝不将 `NaN` 或极端参数送入音频引擎**。
3. **自动防削波净空计算（Auto Headroom Protection）**：
   - 若用户导入的 EQ 或是手动叠加了 Stage 5 空气感提升（$+G_{\text{air}}$），计算总前置增益：
     $$\text{EffectivePreamp}_{\text{dB}} = \text{AutoEQPreamp}_{\text{dB}} - \max(0.0, G_{\text{air}}) + \text{UserPreampTrim}_{\text{dB}}$$
   - 通过 `volume={EffectivePreamp}dB` 置于滤镜链最前端（Stage 1）。
4. **Filter 类型到 `lavfi` 节点的精确映射**：
   - `PK` / `PEQ` $\rightarrow$ `equalizer=f={fc}:width_type=q:width={q}:g={gain}`
   - `LSC` / `LS` $\rightarrow$ `lowshelf=f={fc}:width_type=q:width={q}:g={gain}`
   - `HSC` / `HS` $\rightarrow$ `highshelf=f={fc}:width_type=q:width={q}:g={gain}`

---

## Part 4: Anti-Ear-Blast Protection (防炸耳保护：前瞻软限制器规范)

### 4.1 为什么 ASMR 必须使用带前瞻（Lookahead）的透明软限制器？

在 ASMR 收听（尤其是睡前佩戴入耳式耳机/耳塞）时，用户往往把系统或耳放音量调高 $+10\text{ dB}$ 以上以听清微弱耳语。一旦音频中出现意外掉落道具、拍打麦克风、咳嗽或未经后期压限的突发大笑（瞬态电平直冲 $0\text{ dBFS}$），不仅会造成严重惊吓，更可能损伤听力。
普通无前瞻压缩器（Zero-lookahead Compressor）存在 $5\text{–}20\text{ ms}$ 的启动延迟（Attack lag），会导致突发巨响的**第一个最尖锐波峰直接漏过（Overshoot）**，或者因超快启动产生破音削波（Clipping distortion）。

### 4.2 防炸耳限制器核心参数规范（Stage 7）

| 参数名 | 字段名 | 默认标准值 | 允许调节范围 | 工程与听感原因 |
| --- | --- | --- | --- | --- |
| **启用状态** | `limiter_enabled` | `true`（强烈推荐默认开启或一键守护） | `true` / `false` | 置于 DSP 链路最末端，作为鼓膜安全守门员 |
| **限制天花板阈值 (Ceiling / Threshold)** | `limiter_threshold_db` | **`-1.0 dBFS`** | `-0.5 dBFS` ~ `-12.0 dBFS` | 日常防削波用 `-1.0 dBFS`；提供“夜间助眠安全模式”快捷档 `-3.0 dBFS` / `-6.0 dBFS` |
| **前瞻时间 (Lookahead / Attack)** | `limiter_lookahead_ms` | **`4.0 ms`** | `3.0 ms` ~ `5.0 ms` | 提前 $4\text{ ms}$ 预读波形峰值，使增益衰减包络在波峰到达前平滑降到位，实现零削波失真 |
| **释放时间 (Release)** | `limiter_release_ms` | **`80.0 ms`** | `50.0 ms` ~ `150.0 ms` | 结合 ASC（自动平滑释放），瞬态过后约 $80\text{ ms}$ 平滑恢复，避免产生呼吸抽吸感 |
| **自动增益补偿 (Auto Level)** | `level` | **强制关闭 (`false` / `0`)** | 不可开启 | **铁律**：只压高音、绝不提低音！防止安静段底噪被放大 |
| **声道耦合 (Stereo Link)** | — | **强制开启** | 不可关闭 | 左右耳同步等幅压限，确保双耳声场不偏移 |

### 4.3 dBFS 到线性振幅换算与 `mpv` `lavfi` 映射

FFmpeg 的 `alimiter` 滤镜中 `limit` 参数接收 $[0.0625, 1.0]$ 的线性振幅比例。Rust 后端必须使用精确换算公式：

$$\text{limit\_linear} = 10^{\frac{\text{threshold\_dbfs}}{20.0}}$$

- 当 `threshold_dbfs = -1.0 dBFS` 时：$\text{limit\_linear} = 10^{-1.0 / 20.0} \approx 0.891251$
- 当 `threshold_dbfs = -3.0 dBFS` 时：$\text{limit\_linear} = 10^{-3.0 / 20.0} \approx 0.707946$
- 当 `threshold_dbfs = -6.0 dBFS` 时：$\text{limit\_linear} = 10^{-6.0 / 20.0} \approx 0.501187$

生成的 `lavfi` 末端节点：
```text
alimiter=level_in=1.0:level_out=1.0:limit=0.891251:attack=4.0:release=80.0:asc=1:level=0
```

---

## Part 5: Complete `mpv` Filter Graph Construction & IPC Protocol

### 5.1 滤镜图构建器规范 (`build_lavfi_graph`)

在 Rust 后端构建 `af` 属性字符串时，按以下规则组装：

1. **当 `master_bypass == true` 或没有任何子模块启用时**：
   - 发送 `["set_property", "af", ""]` 清空滤镜链，确保 100% Bit-Perfect 直通。
2. **当启用 DSP 链路时**：
   - 将所有激活的 Stage 按固定顺序用英文逗号 `,` 拼接在单个 `lavfi=[...]` 图内（单图内全程保持浮点采样传递，避免多次进出滤镜图的格式转换开销）：
   ```text
   lavfi=[volume=-1.5dB,highpass=f=50:poles=2:width_type=q:width=0.7071,equalizer=f=105:width_type=q:width=0.70:g=6.6,equalizer=f=6500:width_type=q:width=4.0:g=-3.5,highshelf=f=12000:width_type=q:width=0.7071:g=1.5,alimiter=level_in=1.0:level_out=1.0:limit=0.891251:attack=4.0:release=80.0:asc=1:level=0]
   ```
3. **新音轨加载保持（Persistence across `loadfile`）**：
   - `mpv` 的全局 `af` 属性在 `loadfile` 切歌时保持生效；但在 `MpvController::ensure_started` 新建或崩溃重启 `mpv` 子进程后，必须立即将当前持久化的 DSP 滤镜链重新同步至 `mpv`。

### 5.2 WASAPI 独占输出模式（WASAPI Exclusive Mode & Native Sample Rate Pass-Through）

1. **声学原理与共享模式缺陷**：
   - Windows 默认的 **WASAPI 共享模式（Shared Mode）** 会将所有应用的音频流送入系统混音器（`audiodg.exe`）强制重采样（通常统一锁死为 $48\text{ kHz}$），这不仅破坏了 $44.1\text{ kHz}$ / $96\text{ kHz}$ / $192\text{ kHz}$ 原生整数采样率，还会引入重采样抖动（Jitter）、混音器限幅染色和微弱底噪，导致 ASMR 录音中极其细腻的耳道空间感被抹平。
2. **WASAPI 独占模式规范**：
   - 通过 `mpv` 参数与运行时属性控制：
     - 固定音频输出后端：`--ao=wasapi`
     - 独占模式开关：`["set_property", "audio-exclusive", true / false]`（或启动参数 `--audio-exclusive=yes/no`）
     - 指定声卡/DAC 设备：`["set_property", "audio-device", "<wasapi_device_id>"]`（通过读取 `audio-device-list` 枚举展示）
   - 开启独占模式后，播放器直接接管 DAC / 声卡硬件时钟：源文件是 $96\text{ kHz}/24\text{-bit}$ 就原汁原味按 $96\text{ kHz}/24\text{-bit}$ 输出，背景黑度与微动态解析力达到最佳。
   - **独占冲突优雅回退（Fail-Safe Fallback）**：若目标声卡正被其他独占程序占用或硬件不支持当前采样率导致输出初始化失败，`MpvController` 必须捕获错误、自动回退至 `audio-exclusive=no`（共享模式）并通过 `AppError` 提示用户，严禁卡死播放队列。

### 5.3 超大文件定长流式环形缓冲（Bounded Streaming Ring Buffer — 50~100MB 内存封顶）

1. **长音频内存红线**：
   - ASMR 音轨动辄单条持续 $1\text{–}3\text{ 小时}$（$96\text{ kHz}/24\text{-bit}$ WAV/FLAC 单文件体积高达数百 MB 乃至 $2\text{ GB}+$）。
   - **严禁**像普通 Web 播放器那样把整个音频文件或整条解码后的 `ArrayBuffer` / PCM 一次性读入内存。
2. **定长流式环形缓冲机制（Ring Buffer）**：
   - 在 Rust / `mpv` 引擎层强制采用定长环形流缓冲区（Demuxer Forward + Back Ring Buffer），边按块流式读盘/拉流、边送入解码与浮点 DSP、边覆写过期块：
     - `--cache=yes`
     - `--demuxer-max-bytes=64MiB`（前向环形预读上限 $64\text{ MB}$）
     - `--demuxer-max-back-bytes=32MiB`（后向环形回溯上限 $32\text{ MB}$，保障 A–B 循环与短距快退零延迟命中内存，同时总环形缓冲严格控制在 $\le 96\text{ MB}$）
   - **性能指标**：无论播放 $30\text{ 分钟}$ 还是 $3\text{ 小时}$ 的 GB 级无损音频，音频引擎常驻内存占用始终稳定压在 **`50 MB` 以内（峰值绝不超过 `100 MB`）**。

### 5.4 极速冷启动、毫秒级起播（`< 30 ms` Time-to-First-Audio）与全链路硬件加速

1. **应用秒开与 `mpv` 后台静默预热（Background Pre-Warm）**：
   - 应用启动（`setup`）阶段严禁在主线程执行耗时磁盘遍历或网络请求；下载自愈（`reconcile_startup`）等任务异步下放。
   - 主窗口完成挂载后，后台立即静默调用 `MpvController::ensure_started()` 预热 `mpv` 子进程、建立命名管道并批量完成属性订阅与 DSP 滤镜初始化。用户首次点击“播放”时无需等待子进程创建与管道握手（消除 $200\text{–}500\text{ ms}$ 首播冷启动延迟）。
2. **IPC 管道批处理与极速解复用起播（Fast Demuxer Probing & Single-Roundtrip `loadfile`）**：
   - **合并串行 IPC 往返**：`ensure_started` 中的 `observe_property` 订阅采用流水线批量写入而不逐条串行阻塞等待；`load_file` 避免分多次串行发送 `http-header-fields`、`pause`、`loadfile`，減少 IPC 往返延迟。
   - **跳过封面视频流探测与缩小探测窗口**：ASMR 音频文件（如 FLAC/MP3/M4A）常内嵌数 MB 的高清封面图，FFmpeg 默认会将其当作 MJPEG/PNG 视频流探测。启动参数必须显式加入：
     - `--vid=no`、`--vd=no`、`--sub=no`（彻底跳过视频流与内嵌字幕流探测）
     - `--demuxer-lavf-analyzeduration=0.5`、`--demuxer-lavf-probesize=262144`（将容器探测上限收敛至 $256\text{ KB}$ / $0.5\text{ s}$，实现本地文件 **`< 30 ms` 瞬时出声**，在线流起播提速 $2\times\text{–}3\times$）
     - `--gapless-audio=weak`（保持音频输出设备热衔接，减少切歌时 DAC 休眠唤醒延迟与爆音）
3. **全链路硬件加速（GPU 界面合成 + 媒体解码/SIMD 硬件加速）**：
   - **WebView2 GPU 硬件合成加速**：当设置项 `hardware_acceleration = true`（默认开启）时，为 WebView2 注入 `--enable-gpu-rasterization --enable-zero-copy --ignore-gpu-blocklist`，确保沉浸式播放页的 `blur(72px)` 动态背景、左下角大封面过渡动效与 60/120 FPS 交互式 PEQ 画布完全由 GPU 硬件加速合成。
   - **`mpv` 媒体硬解与多线程 SIMD 加速**：启用 `--hwdec=auto-safe` 与 `--ad-lavc-threads=0`（自动匹配 CPU 核心数并行解码高码率无损音频，并利用 AVX2/FMA 硬件指令集加速 `lavfi` 浮点滤镜链）；提供设置页开关支持一键降级为软件模式以兼容特殊驱动或远程桌面环境。

### 5.5 专业音频遥测属性订阅（foobar2000 / Neutron Technical Telemetry）

为提供专业播放器级别的精确状态监视，`MpvController` 可订阅或按需读取以下属性并包含在状态快照或诊断面板中：
- `audio-codec-name`（如 `flac`, `pcm_s24le`, `mp3`, `aac`, `opus`）
- `audio-params/samplerate`（如 `44100`, `48000`, `96000` Hz）
- `audio-params/format`（如 `s16`, `s24`, `floatp`）
- `audio-params/channel-count` & `audio-params/channels`（如 `2.0 stereo`）
- `audio-bitrate`（实时音频码率 `kbps`）
- `current-ao` & `audio-exclusive`（如 `WASAPI Exclusive` vs `WASAPI Shared`）
- `demuxer-cache-state`（实时环形缓冲区占用字节与缓冲秒数）
- 当前激活的 DSP 链路摘要（如：`直通原声 (Bypass)` 或 `WASAPI 独占 · HPF 50Hz · 去齿音 -3.5dB · 空气感 +1.5dB · 防炸耳 -1.0dBFS`）

---

## Part 6: Built-In ASMR Curated Presets (内置专业预设标准)

除了支持用户自定义与 AutoEQ 导入外，播放器应提供 4 个经过声学调校的开箱即用预设：

| 预设 ID | 标准显示名称 | 适用场景说明 | 预设参数组合 |
| --- | --- | --- | --- |
| `bypass` | **原声直通 (Bit-Perfect)** | 不做任何滤波与压限，100% 还原录音原貌 | 全模块关闭 (`af=""`) |
| `asmr_pure_guard` | **纯净防炸耳 (推荐日常)** | 仅滤除极低频麦克风震动并开启 $-1\text{ dBFS}$ 防炸耳保护，不改变人声音色 | HPF: `ON (50 Hz)` · De-Esser: `OFF` · Air: `OFF` · Limiter: `ON (-1.0 dBFS, 4 ms)` |
| `asmr_de_esser` | **柔和去齿音 (口腔音/掏耳)** | 针对高刺激咀嚼音、舌音、竹签敲击，显著降低 $6.5\text{ kHz}$ 听觉疲劳 | HPF: `ON (55 Hz)` · De-Esser: `ON (6500 Hz, Q 4.0, -4.5 dB)` · Air: `OFF` · Limiter: `ON (-1.5 dBFS, 4 ms)` |
| `asmr_whisper_sleep` | **贴耳助眠耳语 (夜间深度)** | 增强呼吸空气感、柔化齿音，并将防炸耳天花板收紧至 $-3.0\text{ dBFS}$ 安心入睡 | HPF: `ON (50 Hz)` · De-Esser: `ON (6500 Hz, Q 4.0, -3.0 dB)` · Air: `ON (12000 Hz, +1.5 dB)` · Limiter: `ON (-3.0 dBFS, 4.5 ms)` |

---

## Part 7: Interactive Parametric EQ (交互式 PEQ 动态曲线) & Control Surface Standards

相比传统固定几根滑块的图形均衡器（Graphic EQ），ASMR 调音高度依赖对特定刺耳频点（如某个道具在 $6320\text{ Hz}$ 的尖峰）进行外科手术式的精确捕捉。前端必须实现类似 **FabFilter Pro-Q / Neutron** 的**可拖拽交互式参数均衡器（Interactive PEQ Canvas）**。

### 7.1 交互式 PEQ 坐标系与三自由度手势规范

1. **对数频率横轴 $\leftrightarrow$ 像素坐标双向映射**（$f \in [20\text{ Hz}, 20000\text{ Hz}]$）：
   $$x(f) = W \cdot \frac{\log_{10}(f / 20)}{\log_{10}(1000)} = \frac{W}{3} \log_{10}\left(\frac{f}{20}\right), \qquad f(x) = 20 \cdot 10^{\frac{3x}{W}}$$
2. **线性分贝纵轴 $\leftrightarrow$ 像素坐标双向映射**（$G \in [-15\text{ dB}, +15\text{ dB}]$）：
   $$y(G) = \frac{H}{2} \left(1 - \frac{G}{15}\right), \qquad G(y) = 15 \cdot \left(1 - \frac{2y}{H}\right)$$
3. **三自由度直接操纵（Direct Manipulation）**：
   - **点击坐标轴空白处**：在该 $(f, G)$ 位置新增一个 PEQ 控制锚点（Node）；
   - **水平拖拽控制点（Pointer Move X）**：连续调整该频段的中心频率 `Frequency` ($f_0$)；
   - **垂直拖拽控制点（Pointer Move Y）**：连续调整该频段的增益 `Gain` ($G$，单位 $\text{dB}$)；
   - **在控制点上滚动鼠标滚轮（Mouse Wheel）**：连续调整该频段的品质因数/带宽 `Q`（向上滚增大 $Q$ 收窄带宽，向下滚减小 $Q$ 拓宽带宽，范围 $[0.1, 20.0]$）；
   - **双击控制点**：将该点增益归零（$0.0\text{ dB}$）或删除自定义节点。

### 7.2 前端传递函数实时计算与轻量三元组通信

1. **前端本地计算二阶双二阶传递函数（Audio EQ Cookbook Biquad Transfer Function）**：
   - 前端无需等待后端返回曲线点，直接根据每个控制点的 3 个核心数值 $(f_0, G, Q)$ 与滤波器类型（`HPF` / `PK` / `LSC` / `HSC`），按采样率 $f_s = 48000\text{ Hz}$ 计算归一化数字角频率 $\omega_0 = \frac{2\pi f_0}{f_s}$、$\alpha = \frac{\sin\omega_0}{2Q}$、$A = 10^{G/40}$ 及归一化系数 $(b_0, b_1, b_2, 1, a_1, a_2)$。
   - 对画布横轴上 $256\text{–}512$ 个对数采样频点 $\omega$，利用欧拉公式直接求幅频响应分贝值：
     $$|H(e^{j\omega})|_{\text{dB}} = 10 \log_{10} \left( \frac{(b_0 + b_1\cos\omega + b_2\cos 2\omega)^2 + (b_1\sin\omega + b_2\sin 2\omega)^2}{(1 + a_1\cos\omega + a_2\cos 2\omega)^2 + (a_1\sin\omega + a_2\sin 2\omega)^2} \right)$$
   - 各段滤波器分贝响应加上 `Preamp` 求和即得总合成曲线，前端以 60/120 FPS 实时渲染平滑总曲线与当前选中节点的半透明带宽阴影。
2. **前后端极简三元组同步**：
   - 拖拽或滚轮调节时，前端画面立即 0 延迟响应，同时经过 `40ms` 节流（Throttle）仅需将节点的 **`(frequency_hz, gain_db, q_factor)`** 数值传给 Rust 后端更新 `lavfi` 滤镜链，实现极致流畅的交互。

### 7.3 面板读数与文案标准

1. **精确数值读数与等宽字体**：
   - 每个控制点浮层与底部参数栏必须实时显示带单位的精确数值（如 `6500 Hz`、`-3.5 dB`、`Q 4.00`、`-1.0 dBFS`、`4.0 ms`），且必须应用 `font-variant-numeric: tabular-nums`。
   - 增益正值必须显式带 `+` 号（如 `+1.5 dB`），零值显示为 `0.0 dB`。
2. **标准按钮与标签用词**：
   - 面板主标题：`专业音效与防炸耳 (DSP / PEQ)`
   - 核心模块与输出模式标准用词：
     - `WASAPI 独占直通 (Exclusive Mode)` / `WASAPI 共享模式 (Shared Mode)`
     - `超低频截断 (Low-Cut)`
     - `齿音与刺耳压制 (De-Esser)`
     - `贴耳空气感 (Air Boost)`
     - `防炸耳峰值限制器 (Peak Limiter)`
   - AutoEQ 操作按钮：`导入 AutoEQ 配置`、`粘贴 ParametricEQ 文本`、`清除 AutoEQ 校准`。

---

## Part 8: Verification Checklist for Audio & DSP Work

1. **数学与滤镜图单元测试（Rust & TS）**：
   - 验证 `Bypass` 模式下生成的滤镜字符串为空 `""`。
   - 验证前端 Biquad 传递函数在中心频率 $f_0$ 处的计算增益 $|H(e^{j\omega_0})|_{\text{dB}}$ 与设定 `Gain` 误差 $< 0.05\text{ dB}$。
   - 验证 AutoEQ 解析器能正确解析含 `Preamp`、`PK`、`LSC`、`HSC`、`ON`/`OFF` 的标准文本，并对越界频率/Q 值返回 `ErrorCode::InvalidInput`。
   - 验证防炸耳限制器的 dBFS 转线性振幅精度（如 `-1.0 dBFS` $\rightarrow 0.891251$，且 `level=0` 绝不开启自动增益）。
2. **长音频环形缓冲内存与双声道回归验证（R16 /M8–M11）**：
   - 验证加载 1～3 小时大体积无损音频时，流式环形缓冲区（`64MiB` 前向 + `32MiB` 后向）生效，进程常驻内存稳定保持在 `50MB–100MB` 范围内，无全文件读入内存行为。
   - 使用 `tests/fixtures/audio/stereo_lr_check.mp3` 验证开启全套 DSP 或切换 WASAPI 独占/共享模式后，左右声道不反转、不混音为单声道，拖拽 PEQ 节点实时生效且无爆音。

