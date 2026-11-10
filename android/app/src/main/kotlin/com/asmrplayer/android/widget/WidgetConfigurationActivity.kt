package com.asmrplayer.android.widget

import android.app.Application
import android.appwidget.AppWidgetManager
import android.content.ComponentName
import android.content.Intent
import android.os.Bundle
import android.util.SizeF
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.viewModels
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material3.*
import com.asmrplayer.android.ui.*
import com.asmrplayer.android.UiFeedback
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import androidx.media3.common.util.UnstableApi
import kotlinx.coroutines.*
import kotlin.math.roundToInt

@UnstableApi
class WidgetConfigurationActivity : ComponentActivity() {
    private val model: WidgetEditorModel by viewModels()
    override fun onCreate(savedInstanceState:Bundle?) {
        super.onCreate(savedInstanceState);setResult(RESULT_CANCELED);enableEdgeToEdge()
        val ids=AppWidgetManager.getInstance(this).getAppWidgetIds(ComponentName(this,PlaybackWidget::class.java)).toList()
        val requested=intent.getIntExtra(AppWidgetManager.EXTRA_APPWIDGET_ID,AppWidgetManager.INVALID_APPWIDGET_ID)
        val owned=AppWidgetManager.getInstance(this).getAppWidgetInfo(requested)?.provider==ComponentName(this,PlaybackWidget::class.java)
        val available=if(owned) (listOf(requested)+ids).distinct() else ids
        setContent {
            val dark=androidx.compose.foundation.isSystemInDarkTheme()
            MaterialTheme(colorScheme=if(dark) darkColorScheme(primary=Color.White,onPrimary=Color.Black)
                else lightColorScheme(primary=Color(0xff242428),onPrimary=Color.White)) {
                val host = remember { NoticeHost() }
                NoticeTimer(host)
                LaunchedEffect(model.error) { model.error?.let { host.show(UiFeedback(it)) } }
                CompositionLocalProvider(LocalNoticeHost provides host) {
                NoticeOverlay()
                Editor(model,available,requested.takeIf{owned}?:available.firstOrNull(),onCancel=::finish) { id ->
                    setResult(RESULT_OK,Intent().putExtra(AppWidgetManager.EXTRA_APPWIDGET_ID,id));finish()
                }
                }
            }
        }
    }
}

@UnstableApi
internal class WidgetEditorModel(application:Application):AndroidViewModel(application) {
    var id by mutableIntStateOf(AppWidgetManager.INVALID_APPWIDGET_ID);private set
    var config by mutableStateOf(WidgetGlassConfig());private set
    var loading by mutableStateOf(false);private set
    var error by mutableStateOf<String?>(null);private set
    fun select(next:Int) {
        if(next==id)return
        id=next;loading=true;error=null
        viewModelScope.launch {
            try { val saved=withContext(Dispatchers.IO){WidgetGlassStore.read(getApplication(),next)}
                if(id==next)config=saved }
            catch(e:Exception){error="读取组件配置失败：${e.message}"}
            finally{loading=false}
        }
    }
    fun edit(next:WidgetGlassConfig){config=next}
    fun save(onSaved:(Int)->Unit){loading=true;error=null
        viewModelScope.launch {try {
            withContext(Dispatchers.IO){WidgetGlassStore.save(getApplication(),id,config)}
            PlaybackWidget.refresh(getApplication());onSaved(id)
        }catch(e:Exception){error=e.message?:"保存失败，请重试"}finally{loading=false}}
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@UnstableApi
@Composable
private fun Editor(model:WidgetEditorModel,ids:List<Int>,initial:Int?,onCancel:()->Unit,onSaved:(Int)->Unit) {
    val context=LocalContext.current
    LaunchedEffect(initial){initial?.let(model::select)}
    val manager=AppWidgetManager.getInstance(context)
    val options=remember(model.id){manager.getAppWidgetOptions(model.id)}
    val size=SizeF(options.getInt(AppWidgetManager.OPTION_APPWIDGET_MIN_WIDTH,320).coerceAtLeast(250).toFloat(),
        options.getInt(AppWidgetManager.OPTION_APPWIDGET_MAX_HEIGHT,160).coerceAtLeast(140).toFloat())
    val draft=model.config
    val cover by PlaybackWidget.cover.collectAsState()
    var frame by remember { mutableStateOf<GlassFrame?>(null) }
    var previewing by remember{mutableStateOf(false)}
    LaunchedEffect(draft,size,cover){
        previewing=true
        try {delay(100);frame=withContext(Dispatchers.Default){
            val image=cover?.let(WidgetGlass::decodeCover)
            try{WidgetGlass.render(size.width,size.height,draft,image)}finally{image?.recycle()}
        }}
        finally {previewing=false}
    }
    Scaffold(topBar={TopAppBar(title={Text("组件外观")},navigationIcon={TextButton(onClick=onCancel){Text("返回")}})},
        bottomBar={Surface(tonalElevation=2.dp){Row(Modifier.fillMaxWidth().navigationBarsPadding().padding(16.dp),horizontalArrangement=Arrangement.spacedBy(12.dp)){
            OutlinedButton(onClick=onCancel,enabled=!model.loading){Text("取消")}
            Button(onClick={model.save(onSaved)},enabled=model.id in ids&&!model.loading&&!previewing,modifier=Modifier.weight(1f)){Text(if(model.loading)"处理中…" else "应用到桌面")}
        }}}) { insets ->
        LazyColumn(Modifier.fillMaxSize().padding(insets),contentPadding=PaddingValues(20.dp),verticalArrangement=Arrangement.spacedBy(16.dp)) {
            if(ids.isEmpty()) item {Text("请先在系统媒体设置中添加一个桌面播放组件，再回来配置。")}
            if(ids.size>1) item {Row(horizontalArrangement=Arrangement.spacedBy(8.dp)){
                ids.forEachIndexed { index,id->FilterChip(selected=model.id==id,onClick={if(!model.loading)model.select(id)},label={Text("组件 ${index+1}")})}
            }}
            if(model.loading)item{LoadingRing()}
            item {Text("组件效果",style=MaterialTheme.typography.titleMedium)
                Spacer(Modifier.height(8.dp))
                frame?.let { f ->
                    // Native RemoteViews preview shares the actual widget layout and privacy snapshot.
                    AndroidView(factory={android.widget.FrameLayout(it)},modifier=Modifier.fillMaxWidth().height(size.height.dp).background(androidx.compose.ui.graphics.Brush.linearGradient(listOf(Color(0xff797b87),Color(0xffc6b6ac),Color(0xff5c626b)))),update={host->
                        host.removeAllViews();host.addView(PlaybackWidget.preview(context,f,size,model.id).apply(context,host))
                    })
                }
                Text(if(previewing)"正在更新预览…" else if(cover!=null)"当前封面铺满并模糊，切歌时跟随更新。" else "没有可显示的封面时使用不透明中性背景。",style=MaterialTheme.typography.bodySmall)
            }
            item {Text("封面背景外观",style=MaterialTheme.typography.titleMedium)
                Row(horizontalArrangement=Arrangement.spacedBy(8.dp)){
                    listOf("light" to "浅色", "dark" to "深色").forEach{(mode,label)->
                        FilterChip(selected=draft.mode==mode,onClick={model.edit(draft.copy(mode=mode))},label={Text(label)})}
                }
                Text("深浅用于无封面时的中性背景；封面背景不透出壁纸。增加浓度可让文字更清晰。",style=MaterialTheme.typography.bodySmall)
                EditorSlider("背景浓度",draft.opacity,.08f.. .36f){model.edit(draft.copy(opacity=it))}
                Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.SpaceBetween){
                    Text("实色模式",modifier=Modifier.weight(1f));Switch(checked=draft.solid,onCheckedChange={model.edit(draft.copy(solid=it))})
                }
            }
        }
    }
}

@Composable
private fun EditorSlider(label:String,value:Float,range:ClosedFloatingPointRange<Float>,onChange:(Float)->Unit){
    var editing by remember{mutableStateOf(false)}
    var input by remember{mutableStateOf("")}
    Row(Modifier.fillMaxWidth(),verticalAlignment=androidx.compose.ui.Alignment.CenterVertically,horizontalArrangement=Arrangement.SpaceBetween){
        Text(label)
        TextButton(onClick={input="%.2f".format(java.util.Locale.US,value*100);editing=true},modifier=Modifier.semantics{contentDescription="设置$label"}){Text("${(value*100).roundToInt()}%")}
    }
    Slider(value=value.coerceIn(range.start,range.endInclusive),onValueChange=onChange,valueRange=range,modifier=Modifier.semantics{contentDescription=label})
    if(editing)PlayerAlertDialog(onDismissRequest={editing=false},title={Text(label)},text={
        OutlinedTextField(value=input,onValueChange={input=it},label={Text("百分比")},singleLine=true,keyboardOptions=KeyboardOptions(keyboardType=KeyboardType.Decimal))
    },confirmButton={TextButton(onClick={input.toFloatOrNull()?.takeIf{it.isFinite()}?.let{onChange((it/100).coerceIn(range.start,range.endInclusive));editing=false}},enabled=input.toFloatOrNull()?.isFinite()==true){Text("确定")}},dismissButton={TextButton(onClick={editing=false}){Text("取消")}})
}
