package com.asmrplayer.android.ui

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.*
import androidx.compose.ui.unit.dp
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.PlayerScreenState
import com.asmrplayer.android.PlayerViewModel
import com.asmrplayer.core.*
import java.util.Locale

internal val TonePreset.title get() = when(this) {
    TonePreset.NATURAL -> "自然音色"; TonePreset.SOFT -> "柔和人声"; TonePreset.WHISPER -> "耳语细节"
    TonePreset.RUMBLE -> "低频净化"; TonePreset.WARM -> "温暖柔和"; TonePreset.CLEAR -> "清晰轻语"
}
private val TonePreset.detail get() = when(this) {
    TonePreset.NATURAL -> "不加音色修饰"; TonePreset.SOFT -> "减轻尖锐齿音"; TonePreset.WHISPER -> "轻提空气感"
    TonePreset.RUMBLE -> "减少低频轰鸣"; TonePreset.WARM -> "收敛高频亮度"; TonePreset.CLEAR -> "低频净化与气音细节"
}
private val TonePreset.glyph get() = when(this) {
    TonePreset.NATURAL -> Glyph.Audio; TonePreset.SOFT -> Glyph.Heart; TonePreset.WHISPER -> Glyph.Discover
    TonePreset.RUMBLE -> Glyph.Sliders; TonePreset.WARM -> Glyph.Sleep; TonePreset.CLEAR -> Glyph.Eye
}

@Composable
internal fun TonePresetCards(config:DspConfig, enabled:Boolean, update:(DspConfig)->Unit, showHeading:Boolean = true) {
    Column(verticalArrangement=Arrangement.spacedBy(8.dp)) {
        if (showHeading) Row(Modifier.fillMaxWidth(),verticalAlignment=Alignment.CenterVertically) {
            Text("声音预设",Modifier.weight(1f).alignByBaseline(),style=MaterialTheme.typography.titleSmall)
            Text(if(config.bypass)"音效已关闭" else "当前：${TonePreset.entries.firstOrNull{it.matches(config)}?.title?:"自定义"}",
                modifier=Modifier.alignByBaseline(),style=MaterialTheme.typography.labelMedium,color=MaterialTheme.colorScheme.onSurfaceVariant)
        }
        TonePreset.entries.chunked(2).forEach { pair ->
            Row(horizontalArrangement=Arrangement.spacedBy(10.dp)) {
                pair.forEach { preset ->
                    val selected=preset.matches(config)
                    val colors=MaterialTheme.colorScheme
                    Surface(onClick={update(preset.applyTo(config))},enabled=enabled,
                        modifier=Modifier.weight(1f).semantics { this.selected=selected;role=Role.RadioButton },
                        shape=RoundedCornerShape(16.dp),color=motionColor(if(selected)colors.primaryContainer else colors.surfaceContainerHigh),
                        border=BorderStroke(1.dp,motionColor(if(selected)colors.primary.copy(alpha=.65f) else colors.outlineVariant.copy(alpha=.35f)))) {
                        Column(Modifier.padding(12.dp),verticalArrangement=Arrangement.spacedBy(5.dp)) {
                            Row(verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(8.dp)) {
                                AppGlyph(preset.glyph,Modifier.size(18.dp))
                                Text(preset.title,Modifier.weight(1f),style=MaterialTheme.typography.labelLarge)
                                if(selected)AppGlyph(Glyph.Check,Modifier.size(16.dp),filled=true) else Spacer(Modifier.size(16.dp))
                            }
                            Text(preset.detail,style=MaterialTheme.typography.bodySmall,color=colors.onSurfaceVariant)
                        }
                    }
                }
            }
        }
    }
}

@UnstableApi
@Composable
internal fun PlayerEffectsSheet(state:PlayerScreenState,model:PlayerViewModel) {
    val config=state.dsp
    val enabled=state.connected&&!state.headphonesBusy
    fun update(next:DspConfig)=model.updateDsp(next.copy(bypass=false))
    LazyColumn(Modifier.fillMaxWidth(),contentPadding=PaddingValues(start=20.dp,end=20.dp,top=8.dp,bottom=24.dp),
        verticalArrangement=Arrangement.spacedBy(12.dp)) {
        item { TonePresetCards(config,enabled,::update) }
        item {
            Column(verticalArrangement=Arrangement.spacedBy(4.dp)) {
                ToggleRow("响度均衡","减小同一音轨内的音量起伏",config.leveler&&!config.bypass,enabled){update(config.copy(leveler=it))}
                MotionReveal(config.leveler&&!config.bypass) {
                    DetailDisclosure("参数调整","目标 ${number(config.targetLufs)} LUFS") {
                        EffectSlider("目标响度",config.targetLufs,-30.0..-16.0,"LUFS",enabled){update(config.copy(targetLufs=it))}
                        EffectSlider("最大提升",config.maxBoostDb,0.0..12.0,"dB",enabled){update(config.copy(maxBoostDb=it))}
                        Text("提高增益也可能使底噪更明显。峰值限幅保持开启。",style=MaterialTheme.typography.bodySmall,color=MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
                HorizontalDivider(color=MaterialTheme.colorScheme.outlineVariant)
                ToggleRow("音轨响度统一","减小切换音轨时的音量差",config.normalize&&!config.bypass,enabled){update(config.copy(normalize=it))}
                MotionReveal(config.normalize&&!config.bypass) {
                    Column(verticalArrangement=Arrangement.spacedBy(8.dp)) {
                        Text(state.loudnessStatus.ifBlank{"等待当前音轨分析"},style=MaterialTheme.typography.bodySmall)
                        if(state.loudnessBusy) {
                            LoadingRing(if (state.loudnessProgress >= 0) "${state.loudnessProgress.coerceIn(0,100)}%" else null)
                        }
                        if(state.loudnessStatus.startsWith("整轨响度"))Text("当前补偿 ${number(state.normalizationGainDb)} dB",style=MaterialTheme.typography.labelMedium)
                        DetailDisclosure("分析与参数") {
                            EffectSlider("目标响度",config.targetLufs,-30.0..-16.0,"LUFS",enabled){update(config.copy(targetLufs=it))}
                            EffectSlider("最大提升",config.maxBoostDb,0.0..12.0,"dB",enabled){update(config.copy(maxBoostDb=it))}
                            MotionTextButton({model.analyzeLoudness(state.loudnessBusy)},enabled=enabled&&state.queue.isNotEmpty()) { Text(if(state.loudnessBusy)"取消分析" else "重新分析") }
                        }
                    }
                }
            }
        }
        item {
            HorizontalDivider(color=MaterialTheme.colorScheme.outlineVariant)
            DetailDisclosure("精细调整","音色、声道与均衡器") {
                ToggleRow("减少低频震动",null,config.highPass&&!config.bypass,enabled){update(config.copy(highPass=it))}
                if(config.highPass)EffectSlider("截止频率",config.cutoffHz,20.0..100.0,"Hz",enabled){update(config.copy(cutoffHz=it))}
                ToggleRow("柔和去齿音",null,config.deEsser&&!config.bypass,enabled){update(config.copy(deEsser=it))}
                ToggleRow("空气感",null,config.air&&!config.bypass,enabled){update(config.copy(air=it))}
                ToggleRow("温暖音色","温和收敛高频",config.warm&&!config.bypass,enabled){update(config.copy(warm=it))}
                EffectSlider("左右平衡",config.balanceDb,-6.0..6.0,"dB",enabled){update(config.copy(balanceDb=it))}
                ToggleRow("左右声道交换",null,config.swap&&!config.bypass,enabled){update(config.copy(swap=it))}
                ToggleRow("峰值限幅",if(config.leveler||config.normalize)"响度处理开启时保持生效" else "抑制突发峰值",
                    !config.bypass&&(config.limiter||config.leveler||config.normalize),enabled&&!config.leveler&&!config.normalize){update(config.copy(limiter=it))}
                if(config.limiter||config.leveler||config.normalize)EffectSlider("限幅上限",config.ceilingDb,-12.0..-.5,"dBFS",enabled){update(config.copy(ceilingDb=it))}
                DetailDisclosure("参数均衡器","${config.bands.size}/10 个频段") {
                    EffectSlider("前置增益",config.preampDb,-24.0..12.0,"dB",enabled){update(config.copy(preampDb=it))}
                    InlinePeq(config,enabled,::update)
                }
            }
        }
        item {
            HorizontalDivider(color=MaterialTheme.colorScheme.outlineVariant)
            DetailDisclosure("我的方案","保存或应用完整音效设置") { InlineSoundProfiles(state,model) }
        }
    }
}

@Composable
private fun EffectSlider(title:String,value:Double,range:ClosedFloatingPointRange<Double>,unit:String,enabled:Boolean,change:(Double)->Unit) {
    var drag by remember(value){mutableStateOf<Float?>(null)}
    Column {
        Row(Modifier.fillMaxWidth()) { Text(title,Modifier.weight(1f).alignByBaseline(),style=MaterialTheme.typography.bodyMedium);Text("${number(drag?.toDouble()?:value)} $unit",Modifier.alignByBaseline(),style=MaterialTheme.typography.labelMedium) }
        Slider(drag?:value.toFloat(),{drag=it},enabled=enabled,valueRange=range.start.toFloat()..range.endInclusive.toFloat(),
            onValueChangeFinished={drag?.let{change(kotlin.math.round(it*10.0)/10)};drag=null},modifier=Modifier.semantics{contentDescription=title})
    }
}

@UnstableApi
@Composable
private fun InlineSoundProfiles(state:PlayerScreenState,model:PlayerViewModel) {
    var name by rememberSaveable{mutableStateOf("")}
    val data=state.headphones
    val enabled=state.connected&&state.headphonesReady&&!state.headphonesBusy
    Column(verticalArrangement=Arrangement.spacedBy(8.dp)) {
        if(!state.headphonesReady||state.headphonesBusy)LoadingRing()
        data.profiles.forEach { profile ->
            val selected=data.activeId==profile.id&&!data.modified
            Surface(onClick={model.headphoneAction("select",id=profile.id)},enabled=enabled,
                shape=RoundedCornerShape(12.dp),color=motionColor(if(selected)MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh),
                modifier=Modifier.fillMaxWidth().semantics{this.selected=selected}) {
                Row(Modifier.padding(12.dp),verticalAlignment=Alignment.CenterVertically) {
                    Text(profile.name,Modifier.weight(1f),style=MaterialTheme.typography.bodyMedium)
                    AppGlyph(if(selected)Glyph.Check else Glyph.Play,Modifier.size(20.dp),filled=selected)
                }
            }
        }
        Text("方案包含均衡器、音色、响度与左右平衡。",style=MaterialTheme.typography.bodySmall,color=MaterialTheme.colorScheme.onSurfaceVariant)
        OutlinedTextField(name,{name=it.take(80)},label={Text("新方案名称")},singleLine=true,enabled=enabled,modifier=Modifier.fillMaxWidth())
        MotionButton({model.headphoneAction("save",name=name.trim(),onSuccess={name=""})},enabled=enabled&&name.isNotBlank()&&data.profiles.size<32) { Text("保存当前设置") }
        if(data.profiles.size>=32)Text("已达到 32 个方案。",style=MaterialTheme.typography.bodySmall)
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun InlinePeq(config:DspConfig,enabled:Boolean,update:(DspConfig)->Unit) {
    var editing by rememberSaveable{mutableStateOf<Int?>(null)}
    var importing by rememberSaveable{mutableStateOf(false)}
    var text by rememberSaveable{mutableStateOf("")}
    var error by remember{mutableStateOf<String?>(null)}
    config.bands.forEachIndexed { index,band ->
        Column {
            ToggleRow("频段 ${index+1}","${number(band.frequencyHz)} Hz · ${number(band.gainDb)} dB",band.enabled,enabled) { checked ->
                update(config.copy(bands=config.bands.toMutableList().apply{this[index]=band.copy(enabled=checked)}))
            }
            Row {
                MotionTextButton({editing=if(editing==index)null else index},enabled=enabled){Text("编辑")}
                MotionTextButton({update(config.copy(bands=config.bands.filterIndexed{i,_->i!=index}));editing=null},enabled=enabled){Text("移除")}
            }
            if(editing==index)InlineBandEditor(band,enabled,{editing=null}) { changed ->
                update(config.copy(bands=config.bands.toMutableList().apply{this[index]=changed}));editing=null
            }
        }
    }
    Row {
        MotionTextButton({editing=config.bands.size;importing=false},enabled=enabled&&config.bands.size<10){Text("添加频段")}
        MotionTextButton({importing=!importing;editing=null;error=null},enabled=enabled){Text("导入 AutoEQ")}
    }
    if(editing==config.bands.size&&config.bands.size<10)InlineBandEditor(null,enabled,{editing=null}) { update(config.copy(bands=config.bands+it));editing=null }
    MotionReveal(importing) {
        Column(verticalArrangement=Arrangement.spacedBy(8.dp)) {
            OutlinedTextField(text,{text=it.take(65536)},label={Text("AutoEQ 参数文本")},enabled=enabled,modifier=Modifier.fillMaxWidth().heightIn(max=200.dp))
            error?.let{Text(it,color=MaterialTheme.colorScheme.error)}
            MotionButton({try{update(AutoEq.parse(text,config));text="";importing=false;error=null}catch(e:IllegalArgumentException){error=e.message?:"参数无效"}},enabled=enabled&&text.isNotBlank()){Text("应用参数")}
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun InlineBandEditor(original:PeqBand?,enabled:Boolean,cancel:()->Unit,save:(PeqBand)->Unit) {
    var hz by rememberSaveable(original){mutableStateOf((original?.frequencyHz?:1000.0).toString())}
    var gain by rememberSaveable(original){mutableStateOf((original?.gainDb?:0.0).toString())}
    var q by rememberSaveable(original){mutableStateOf((original?.q?:.7071).toString())}
    var kind by rememberSaveable(original){mutableStateOf(original?.kind?:FilterKind.PEAK)}
    val candidate=try{PeqBand(hz.toDouble(),gain.toDouble(),q.toDouble(),kind,original?.enabled?:true)}catch(_:IllegalArgumentException){null}
    Column(verticalArrangement=Arrangement.spacedBy(8.dp)) {
        FlowRow(horizontalArrangement=Arrangement.spacedBy(6.dp)) { FilterKind.entries.forEach{value->
            ChoiceChip(kind==value,{kind=value},{Text(when(value){FilterKind.PEAK->"峰值";FilterKind.LOW_SHELF->"低架";FilterKind.HIGH_SHELF->"高架"})},enabled=enabled)
        } }
        OutlinedTextField(hz,{hz=it},label={Text("频率 · 20–20000 Hz")},singleLine=true,enabled=enabled,modifier=Modifier.fillMaxWidth())
        OutlinedTextField(gain,{gain=it},label={Text("增益 · −24–24 dB")},singleLine=true,enabled=enabled,modifier=Modifier.fillMaxWidth())
        OutlinedTextField(q,{q=it},label={Text("Q · 0.1–20")},singleLine=true,enabled=enabled,modifier=Modifier.fillMaxWidth())
        Row { MotionTextButton({candidate?.let(save)},enabled=enabled&&candidate!=null){Text("保存频段")};MotionTextButton(cancel){Text("取消")} }
        if(candidate==null)Text("请输入范围内的频率、增益和 Q 值。",style=MaterialTheme.typography.bodySmall,color=MaterialTheme.colorScheme.error)
    }
}

private fun number(value:Double)=String.format(Locale.ROOT,"%.1f",value)
