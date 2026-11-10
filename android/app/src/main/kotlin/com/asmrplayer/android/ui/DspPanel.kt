package com.asmrplayer.android.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.asmrplayer.core.*
import java.util.Locale

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun DspPanel(config: DspConfig, connected: Boolean, update: (DspConfig) -> Unit, error: (String?) -> Unit, headphones: () -> Unit,
    loudnessStatus: String = "", analyzing: Boolean = false, progress: Int = -1, normalizationGainDb: Double = 0.0,
    hasTrack: Boolean = false, analyze: (Boolean) -> Unit = {}) {
    var importDialog by rememberSaveable { mutableStateOf(false) }
    var preset by rememberSaveable { mutableStateOf("") }
    var importError by rememberSaveable { mutableStateOf<String?>(null) }
    var editIndex by rememberSaveable { mutableStateOf<Int?>(null) }
    var section by rememberSaveable { mutableStateOf("basic") }
    var panel by remember { mutableStateOf<String?>(null) }
    var reset by remember { mutableStateOf(false) }
    val active=connected&&!config.bypass
    LazyColumn(contentPadding=PageSpace.padding,verticalArrangement=Arrangement.spacedBy(12.dp)) {
        item { MaintenanceCard {
            DetailDisclosure("声音预设", if(config.bypass) "原声对比中" else TonePreset.entries.firstOrNull { it.matches(config) }?.title ?: "自定义") {
                TonePresetCards(config, connected, update, showHeading = false)
            }
        } }
        item { PageTabs(section,listOf("basic" to "常用调整","advanced" to "精细调整")){section=it} }
        if(section=="basic") {
            item { SettingsGroup("播放方案") {
                DspToggleRow("原声对比","暂时跳过音效，保留所有参数",config.bypass,connected){update(config.copy(bypass=it))}
                SettingsLink("耳机与我的方案","按输出设备管理"){headphones()}
            } }
            item { SettingsGroup("音量起伏") {
                DspToggleRow("实时响度均衡","减小同一音轨内的音量起伏",config.leveler,active){update(config.copy(leveler=it))}
                HorizontalDivider()
                DspToggleRow("音轨响度统一","减小切换音轨时的音量差",config.normalize,active){update(config.copy(normalize=it))}
                MotionTextButton({panel="loudness"}){Text("调整响度 · ${decimal(config.targetLufs)} LUFS")}
                if(analyzing) {LoadingRing();Text("正在分析${if(progress>=0)" · $progress%" else ""}",style=MaterialTheme.typography.bodySmall)}
            } }
            item { SettingsGroup("声音质感") {
                DspToggleRow("减少低频震动","过滤低频轰鸣与碰触声",config.highPass,active){update(config.copy(highPass=it))}
                MotionTextButton({panel="highpass"}){Text("截止频率 · ${config.cutoffHz.toInt()} Hz")}
                HorizontalDivider()
                DspToggleRow("柔和去齿音","减弱尖锐的高频齿音",config.deEsser,active){update(config.copy(deEsser=it))}
                HorizontalDivider()
                DspToggleRow("空气感","轻微提升高频细节",config.air,active){update(config.copy(air=it))}
                HorizontalDivider()
                DspToggleRow("温暖音色","温和收敛高频",config.warm,active){update(config.copy(warm=it))}
            } }
            item { SettingsGroup("声道与峰值") {
                SettingsLink("左右平衡",if(config.balanceDb==0.0)"居中" else "${decimal(config.balanceDb)} dB"){panel="balance"}
                DspToggleRow("峰值限幅",if(config.leveler||config.normalize)"响度均衡开启时保持生效" else "抑制突发峰值",config.limiter||config.leveler||config.normalize,active&&!config.leveler&&!config.normalize){update(config.copy(limiter=it))}
                MotionTextButton({panel="limiter"}){Text("限幅上限 · ${decimal(config.ceilingDb)} dBFS")}
            } }
        } else {
            item { SettingsGroup("增益与声道") {
                SettingsLink("前置增益","${decimal(config.preampDb)} dB"){panel="preamp"}
                SettingsLink("左右平衡","${decimal(config.balanceDb)} dB"){panel="balance"}
                DspToggleRow("左右声道交换",null,config.swap,active){update(config.copy(swap=it))}
            } }
            item {
                Text("参数均衡器 · ${config.bands.size}/10",style=MaterialTheme.typography.titleMedium)
                Text("每个频段可独立开关和编辑",style=MaterialTheme.typography.bodySmall,color=MaterialTheme.colorScheme.onSurfaceVariant)
                FlowRow(horizontalArrangement=Arrangement.spacedBy(8.dp)) {
                    MotionOutlinedButton({editIndex=config.bands.size},enabled=connected&&config.bands.size<10){Text("添加频段")}
                    MotionTextButton({importDialog=true;importError=null},enabled=connected){Text("导入 AutoEQ")}
                }
            }
            items(config.bands.size) {index->
                val band=config.bands[index]
                SettingsGroup("频段 ${index+1}") {
                    DspToggleRow(band.kind.label,"${band.frequencyHz.toInt()} Hz · ${decimal(band.gainDb)} dB · Q ${decimal(band.q)}",band.enabled,active){update(config.copy(bands=config.bands.toMutableList().apply{this[index]=band.copy(enabled=it)}))}
                    Row {MotionTextButton({editIndex=index}){Text("编辑频段")};MotionTextButton({update(config.copy(bands=config.bands.filterIndexed{i,_->i!=index}))}){Text("移除频段")}}
                }
            }
        }
        item {Row {MotionTextButton({panel="help"}){Text("音频处理说明")};MotionTextButton({reset=true}){Text("恢复默认音效")}}}
    }
    if(reset) PlayerAlertDialog(onDismissRequest={reset=false},title={Text("恢复默认音效？")},text={Text("重置当前音效参数，已保存的耳机方案保留。")},confirmButton={MotionTextButton({update(DspConfig());reset=false}){Text("恢复默认")}},dismissButton={MotionTextButton({reset=false}){Text("取消")}})
    panel?.let {kind ->
        PlayerAlertDialog(onDismissRequest={panel=null},title={Text(when(kind){"loudness"->"响度均衡";"highpass"->"减少低频震动";"balance"->"左右平衡";"limiter"->"峰值限幅";"preamp"->"前置增益";else->"音频处理说明"})},text={
            Column(Modifier.heightIn(max=420.dp).verticalScroll(rememberScrollState()),verticalArrangement=Arrangement.spacedBy(16.dp)) {
                when(kind) {
                    "loudness" -> {
                        if(!config.leveler&&!config.normalize) {
                            Text("先开启一项响度调整，再设置目标音量。",style=MaterialTheme.typography.bodyMedium)
                            MotionOutlinedButton({update(config.copy(leveler=true))},enabled=active){Text("开启实时响度均衡")}
                        }
                        if(config.bypass)Text("正在原声对比，结束对比后可调整参数。",style=MaterialTheme.typography.bodySmall)
                        DspSlider("目标响度",config.targetLufs,-30.0..-16.0,"LUFS",active&&(config.leveler||config.normalize)){update(config.copy(targetLufs=it))}
                        DspSlider("最大提升",config.maxBoostDb,0.0..12.0,"dB",active&&(config.leveler||config.normalize)){update(config.copy(maxBoostDb=it))}
                        Text(loudnessStatus.ifBlank{"选择本地或已下载音轨后可分析。"},style=MaterialTheme.typography.bodySmall)
                        if(config.normalize)Text("当前补偿 · ${decimal(normalizationGainDb)} dB")
                        MotionOutlinedButton({analyze(analyzing)},enabled=connected&&hasTrack){Text(if(analyzing)"取消分析" else "重新分析当前音轨")}
                        Text("在线流请先下载再分析。提高音量可能让底噪更明显。",style=MaterialTheme.typography.bodySmall)
                        MotionTextButton({update(config.copy(targetLufs=-23.0,maxBoostDb=6.0))}){Text("重置响度参数")}
                    }
                    "highpass" -> {Text("频率越高，削减的低频越多。")
                        DspSlider("截止频率",config.cutoffHz,20.0..100.0,"Hz",active&&config.highPass){update(config.copy(cutoffHz=it))}
                        MotionTextButton({update(config.copy(cutoffHz=50.0))}){Text("重置为 50 Hz")}}
                    "balance" -> {Text("负值偏左，正值偏右；0 为居中。")
                        DspSlider("声道平衡",config.balanceDb,-6.0..6.0,"dB",active){update(config.copy(balanceDb=it))}
                        MotionTextButton({update(config.copy(balanceDb=0.0))}){Text("恢复居中")}}
                    "limiter" -> {Text("上限越低，突发峰值压制越多。")
                        DspSlider("限幅上限",config.ceilingDb,-12.0..-0.5,"dBFS",active&&(config.limiter||config.leveler||config.normalize)){update(config.copy(ceilingDb=it))}
                        MotionTextButton({update(config.copy(ceilingDb=-1.0))}){Text("重置为 −1 dBFS")}}
                    "preamp" -> {Text("均衡器提升频段时会额外预留净空。")
                        DspSlider("前置增益",config.preampDb,-24.0..12.0,"dB",active){update(config.copy(preampDb=it))}
                        MotionTextButton({update(config.copy(preampDb=0.0))}){Text("重置为 0 dB")}}
                    else -> Text("当前输出链路为 PCM 16 位，内部浮点处理。原声对比跳过音效并保留参数，不保证系统或蓝牙逐位一致。\n\n去齿音：6.5 kHz / −3.5 dB / Q 4.0；空气感：12 kHz / +1.5 dB。超过当前音频 Nyquist 频率的滤波器会跳过。\n\n双声道联动限幅；响度均衡开启时限幅保持生效。")
                }
            }
        },confirmButton={MotionTextButton({panel=null}){Text("完成")}})
    }
    if (importDialog) PlayerAlertDialog(onDismissRequest = { importDialog = false }, title = { Text("导入 AutoEQ 参数") }, text = {
        Column {
            Text("粘贴 Equalizer APO / ParametricEQ.txt 内容，支持 Preamp、PK、LS/LSC、HS/HSC，最多 10 段。")
            OutlinedTextField(preset, { preset = it }, label = { Text("预设文本") }, modifier = Modifier.fillMaxWidth().heightIn(max = 240.dp))
            importError?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        }
    }, confirmButton = { MotionTextButton({
        try { update(AutoEq.parse(preset, config)); importDialog = false; preset = "" }
        catch (e: IllegalArgumentException) { importError = e.message ?: "参数无效，请检查频率、增益和 Q 值。" }
    }) { Text("导入预设") } }, dismissButton = { MotionTextButton({ importDialog = false }) { Text("取消") } })
    editIndex?.let { index ->
        BandEditor(config.bands.getOrNull(index), { editIndex = null }) { band ->
            if (index <= config.bands.size) {
                val bands = config.bands.toMutableList()
                if (index == bands.size) bands.add(band) else bands[index] = band
                update(config.copy(bands = bands))
            } else error("频段已发生变化，请重新打开编辑。")
            editIndex = null
        }
    }
}

@Composable
private fun DspSlider(title: String, value: Double, range: ClosedFloatingPointRange<Double>, unit: String, enabled: Boolean, changed: (Double) -> Unit) {
    var drag by remember { mutableStateOf<Float?>(null) }
    var input by remember(value) { mutableStateOf(decimal(value)) }
    val parsed=input.toDoubleOrNull()?.takeIf {it.isFinite()&&it in range}
    Column {
        Text("$title · ${decimal(drag?.toDouble() ?: value)} $unit")
        Slider(value = drag ?: value.toFloat(), onValueChange = { drag = it }, onValueChangeFinished = { drag?.let { changed(it.toDouble()) }; drag = null }, valueRange = range.start.toFloat()..range.endInclusive.toFloat(), enabled = enabled, modifier = Modifier.semantics { contentDescription = title })
        OutlinedTextField(input,{input=it},label={Text("精确数值 · $unit")},singleLine=true,enabled=enabled,isError=parsed==null,
            keyboardOptions=androidx.compose.foundation.text.KeyboardOptions(keyboardType=androidx.compose.ui.text.input.KeyboardType.Decimal),
            trailingIcon={MotionTextButton({parsed?.let(changed)},enabled=enabled&&parsed!=null&&parsed!=value){Text("应用")}},modifier=Modifier.fillMaxWidth())
        if(parsed==null)Text("范围 ${range.start}–${range.endInclusive} $unit",style=MaterialTheme.typography.bodySmall,color=MaterialTheme.colorScheme.error)
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun BandEditor(original: PeqBand?, dismiss: () -> Unit, save: (PeqBand) -> Unit) {
    var hz by rememberSaveable { mutableStateOf((original?.frequencyHz ?: 1000.0).toString()) }
    var gain by rememberSaveable { mutableStateOf((original?.gainDb ?: 0.0).toString()) }
    var q by rememberSaveable { mutableStateOf((original?.q ?: 0.7071).toString()) }
    var kind by rememberSaveable { mutableStateOf(original?.kind ?: FilterKind.PEAK) }
    var error by remember { mutableStateOf<String?>(null) }
    PlayerAlertDialog(onDismissRequest = dismiss, title = { Text(if (original == null) "添加 EQ 频段" else "编辑 EQ 频段") }, text = {
        Column(Modifier.heightIn(max = 360.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(4.dp)) { FilterKind.entries.forEach { k -> ChoiceChip(kind == k, { kind = k }, { Text(k.label) }) } }
            OutlinedTextField(hz, { hz = it }, label = { Text("频率 · 20–20000 Hz") }, singleLine = true)
            OutlinedTextField(gain, { gain = it }, label = { Text("增益 · −24–24 dB") }, singleLine = true)
            OutlinedTextField(q, { q = it }, label = { Text("Q · 0.1–20") }, singleLine = true)
            error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        }
    }, confirmButton = { MotionTextButton({
        try { save(PeqBand(hz.toDouble(), gain.toDouble(), q.toDouble(), kind, original?.enabled ?: true)) }
        catch (_: IllegalArgumentException) { error = "请输入范围内的频率、增益和 Q 值。" }
    }) { Text("保存频段") } }, dismissButton = { MotionTextButton(dismiss) { Text("取消") } })
}

private val FilterKind.label get() = when (this) { FilterKind.PEAK -> "峰值"; FilterKind.LOW_SHELF -> "低架"; FilterKind.HIGH_SHELF -> "高架" }
private fun decimal(value: Double) = String.format(Locale.ROOT, "%.1f", value)

@Composable
private fun DspToggleRow(title:String,detail:String?,checked:Boolean,enabled:Boolean,onChange:(Boolean)->Unit) {
    Box(Modifier.padding(horizontal=16.dp,vertical=8.dp)) { ToggleRow(title,detail,checked,enabled,onChange) }
}
