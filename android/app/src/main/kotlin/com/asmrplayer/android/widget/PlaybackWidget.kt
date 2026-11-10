package com.asmrplayer.android.widget

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.graphics.*
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.LruCache
import android.util.SizeF
import android.widget.RemoteViews
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.MainActivity
import com.asmrplayer.android.R
import com.asmrplayer.android.data.AndroidBackend
import com.asmrplayer.android.data.WidgetCollectionState
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import java.util.concurrent.atomic.AtomicLong

/** Media3 owns playback. This is a volatile, privacy-sanitized display snapshot only. */
@UnstableApi
class PlaybackWidget : AppWidgetProvider() {
    override fun onReceive(context:Context,intent:Intent) {
        if(intent.action==ACTION){
            val pending=goAsync()
            WidgetActions.dispatch(context,intent.getStringExtra(OP),intent.getStringExtra(VERSION),
                if(intent.hasExtra(FAVORITE))intent.getBooleanExtra(FAVORITE,false) else null){pending.finish()}
        } else super.onReceive(context,intent)
    }
    override fun onUpdate(context:Context,manager:AppWidgetManager,ids:IntArray)=update(context)
    override fun onAppWidgetOptionsChanged(context:Context,manager:AppWidgetManager,id:Int,options:android.os.Bundle)=update(context)
    private fun update(context:Context){val pending=goAsync();refresh(context){pending.finish()}}
    override fun onDeleted(context:Context,ids:IntArray){
        val pending=goAsync();scope.launch{try{ids.forEach{WidgetGlassStore.delete(context,it)};cache.evictAll()}finally{pending.finish()}}
    }
    companion object {
        const val ACTION="com.asmrplayer.WIDGET_CONTROL"
        const val OP="widgetOperation"
        const val VERSION="widgetVersion"
        const val FAVORITE="widgetFavorite"
        private data class Snapshot(val title:String="ASMR Player",val artist:String="",val playing:Boolean=false,
            val available:Boolean=false,val position:Long=0,val duration:Long=0,val artwork:ByteArray?=null,val versionId:String?=null,val hasCover:Boolean=false,val playRequested:Boolean=false)
        @Volatile private var snapshot=Snapshot()
        private val currentCover=MutableStateFlow<ByteArray?>(null)
        internal val cover=currentCover.asStateFlow()
        @Volatile private var collection:WidgetCollectionState?=null
        private var collectionJob:Job?=null
        private var observedVersion:String?=null
        private val scope=CoroutineScope(SupervisorJob()+Dispatchers.Default.limitedParallelism(1))
        private val generation=AtomicLong()
        private val cache=object:LruCache<String,GlassFrame>(6*1024*1024){override fun sizeOf(key:String,value:GlassFrame)=value.bitmap.allocationByteCount}
        private val handler=Handler(Looper.getMainLooper())
        private var tick:Runnable?=null
        fun publish(context:Context,player:Player?) {
            tick?.let(handler::removeCallbacks);tick=null
            snapshot=if(player==null)snapshot.copy(playing=false,playRequested=false) else Snapshot(
                player.mediaMetadata.title?.toString()?.takeIf{it.isNotBlank()}?:"ASMR Player",
                player.mediaMetadata.artist?.toString().orEmpty(),player.isPlaying,player.mediaItemCount>0,
                player.currentPosition.coerceAtLeast(0),player.duration.coerceAtLeast(0),player.mediaMetadata.artworkData,player.currentMediaItem?.mediaId,
                player.mediaMetadata.extras?.getBoolean("hasWorkArtwork",false)==true,player.playWhenReady && player.playbackState!=Player.STATE_ENDED)
            currentCover.value=snapshot.artwork.takeIf{snapshot.hasCover}
            val app=context.applicationContext
            val version=snapshot.versionId
            if(version!=observedVersion){
                collectionJob?.cancel();collection=null;observedVersion=version
                collectionJob=version?.let{scope.launch {
                    AndroidBackend.get(app).database.collections().observeWidgetCollection(it).distinctUntilChanged().collect { value ->
                        if(snapshot.versionId==it){collection=value;refresh(app)}
                    }
                }}
            }
            refresh(app)
            if(player?.isPlaying==true && AppWidgetManager.getInstance(app).getAppWidgetIds(ComponentName(app,PlaybackWidget::class.java)).isNotEmpty()){
                // Actual position, 15-second glance updates only while playing; no frame loop or second clock.
                tick=Runnable{publish(app,player)}.also{handler.postDelayed(it,15_000)}
            }
        }
        fun refresh(context:Context,finished:(()->Unit)?=null){
            val revision=generation.incrementAndGet();val app=context.applicationContext
            scope.launch {try{render(app,revision)}catch(e:Exception){android.util.Log.e("PlaybackWidget","Widget update failed",e)}finally{finished?.invoke()}}
        }
        private fun render(context:Context,revision:Long){
            if(revision!=generation.get())return
            val manager=AppWidgetManager.getInstance(context)
            val ids=manager.getAppWidgetIds(ComponentName(context,PlaybackWidget::class.java))
            val state=snapshot
            val coverBytes=state.artwork.takeIf{state.hasCover}
            val coverKey=coverBytes?.contentHashCode()
            ids.forEach{id->
                WidgetGlassStore.removeLegacyImage(context,id)
                val config=WidgetGlassStore.read(context,id);val options=manager.getAppWidgetOptions(id)
                val portrait=SizeF(options.getInt(AppWidgetManager.OPTION_APPWIDGET_MIN_WIDTH,320).coerceAtLeast(250).toFloat(),
                    options.getInt(AppWidgetManager.OPTION_APPWIDGET_MAX_HEIGHT,160).coerceAtLeast(140).toFloat())
                val landscape=SizeF(options.getInt(AppWidgetManager.OPTION_APPWIDGET_MAX_WIDTH,320).coerceAtLeast(250).toFloat(),
                    options.getInt(AppWidgetManager.OPTION_APPWIDGET_MIN_HEIGHT,160).coerceAtLeast(140).toFloat())
                fun views(size:SizeF):RemoteViews {
                    val key="$id:${config.revision}:${size.width}:${size.height}:${state.versionId}:$coverKey"
                    val frame=cache.get(key)?:run{
                        val cover=coverBytes?.let(WidgetGlass::decodeCover)
                        try {WidgetGlass.render(size.width.coerceAtLeast(100f),size.height.coerceAtLeast(100f),config,cover).also{cache.put(key,it)}}
                        finally{cover?.recycle()}
                    }
                    return createViews(context,frame,size,id,state)
                }
                val remote=if(Build.VERSION.SDK_INT>=31){
                    @Suppress("DEPRECATION") val sizes=options.getParcelableArrayList<SizeF>(AppWidgetManager.OPTION_APPWIDGET_SIZES)
                    RemoteViews((sizes?.takeIf{it.isNotEmpty()}?:listOf(portrait,landscape)).distinct().take(4).associateWith(::views))
                }else RemoteViews(views(landscape),views(portrait))
                if(revision==generation.get())manager.updateAppWidget(id,remote)
            }
        }
        internal fun preview(context:Context,frame:GlassFrame,size:SizeF,id:Int):RemoteViews=createViews(context,frame,size,id,snapshot,interactive=false)
        private fun createViews(context:Context,frame:GlassFrame,size:SizeF,id:Int,state:Snapshot,interactive:Boolean=true):RemoteViews {
            val views=RemoteViews(context.packageName,R.layout.playback_widget)
            views.setImageViewBitmap(R.id.widget_glass,frame.bitmap)
            views.setTextViewText(R.id.widget_title,state.title)
            val status=if(!state.available)"打开播放器继续收听" else if(state.playing)"正在播放" else "已暂停"
            views.setTextViewText(R.id.widget_status,if(state.artist.isBlank())status else "${state.artist} · $status")
            views.setTextColor(R.id.widget_title,frame.ink);views.setTextColor(R.id.widget_status,frame.secondary)
            views.setTextViewText(R.id.widget_elapsed,time(state.position))
            views.setTextViewText(R.id.widget_remaining,if(state.duration>0)"−${time((state.duration-state.position).coerceAtLeast(0))}" else "--:--")
            views.setTextColor(R.id.widget_elapsed,frame.secondary);views.setTextColor(R.id.widget_remaining,frame.secondary)
            views.setImageViewBitmap(R.id.widget_progress,progress(state.position,state.duration,frame.ink))
            views.setContentDescription(R.id.widget_progress,if(state.duration>0)"进度 ${time(state.position)}，总时长 ${time(state.duration)}" else "时长未知")
            views.setImageViewResource(R.id.widget_play,if(state.playRequested)R.drawable.widget_pause else R.drawable.widget_play)
            views.setContentDescription(R.id.widget_play,if(state.playRequested)"暂停" else "播放")
            val saved=collection?.takeIf{it.versionId==state.versionId}
            views.setImageViewResource(R.id.widget_favorite,if(saved?.favorite==true)R.drawable.widget_heart_filled else R.drawable.widget_heart)
            views.setContentDescription(R.id.widget_favorite,if(saved?.favorite==true)"取消书签" else "添加书签")
            views.setImageViewResource(R.id.widget_library,if(saved?.inLibrary==true)R.drawable.widget_library_saved else R.drawable.widget_library_add)
            views.setContentDescription(R.id.widget_library,if(saved?.inLibrary==true)"已加入音频库" else "加入音频库")
            listOf(R.id.widget_previous,R.id.widget_play,R.id.widget_next,R.id.widget_configure,R.id.widget_favorite,R.id.widget_library).forEach{
                views.setInt(it,"setColorFilter",if(it==R.id.widget_play)Color.rgb(48,48,58) else frame.ink);views.setInt(it,"setImageAlpha",when(it){R.id.widget_play->255;R.id.widget_configure->198;else->220})
            }
            listOf(R.id.widget_favorite,R.id.widget_library).forEach {
                views.setBoolean(it,"setEnabled",saved!=null)
                if(saved==null)views.setInt(it,"setImageAlpha",85)
            }
            val art=state.artwork?.let(::artwork)
            if(art==null){views.setImageViewResource(R.id.widget_artwork,R.drawable.widget_artwork);views.setInt(R.id.widget_artwork,"setColorFilter",frame.ink)}
            else {views.setInt(R.id.widget_artwork,"setColorFilter",Color.TRANSPARENT);views.setImageViewBitmap(R.id.widget_artwork,art)}
            if (interactive) {
            val open=Intent(context,MainActivity::class.java).setAction(MainActivity.OPEN_PLAYER)
                .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
            views.setOnClickPendingIntent(R.id.widget_root,PendingIntent.getActivity(context,0,open,PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE))
            listOf(R.id.widget_previous to "previous",R.id.widget_play to if(state.playRequested)"pause" else "play",R.id.widget_next to "next",R.id.widget_favorite to "favorite",R.id.widget_library to "library").forEach{(view,op)->
                val intent=Intent(context,PlaybackWidget::class.java).setAction(ACTION).putExtra(OP,op).putExtra(VERSION,state.versionId)
                    .putExtra(FAVORITE,saved?.favorite!=true)
                    .setData(android.net.Uri.Builder().scheme("asmr-widget").authority(op).appendPath(state.versionId.orEmpty()).build())
                views.setOnClickPendingIntent(view,PendingIntent.getBroadcast(context,view,intent,PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE))
            }
            val configure=Intent(context,WidgetConfigurationActivity::class.java).putExtra(AppWidgetManager.EXTRA_APPWIDGET_ID,id)
            views.setOnClickPendingIntent(R.id.widget_configure,PendingIntent.getActivity(context,id,configure,PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE))
            }
            return views
        }
        private fun time(ms:Long):String {val seconds=ms/1000;return if(seconds>=3600)"${seconds/3600}:${(seconds/60%60).toString().padStart(2,'0')}:${(seconds%60).toString().padStart(2,'0')}" else "${seconds/60}:${(seconds%60).toString().padStart(2,'0')}"}
        private fun progress(position:Long,duration:Long,color:Int):Bitmap {
            val b=Bitmap.createBitmap(500,6,Bitmap.Config.ARGB_8888);val c=Canvas(b);val p=Paint(Paint.ANTI_ALIAS_FLAG)
            p.color=color;p.alpha=45;c.drawRoundRect(0f,0f,500f,6f,3f,3f,p)
            if(duration>0){p.alpha=220;c.drawRoundRect(0f,0f,500f*(position.toDouble()/duration).coerceIn(0.0,1.0).toFloat(),6f,3f,3f,p)}
            return b
        }
        private fun artwork(bytes:ByteArray):Bitmap? {
            val bounds=BitmapFactory.Options().apply{inJustDecodeBounds=true};BitmapFactory.decodeByteArray(bytes,0,bytes.size,bounds)
            if(bounds.outWidth<=0||bounds.outHeight<=0)return null
            var sample=1;while(maxOf(bounds.outWidth,bounds.outHeight)/sample>160)sample*=2
            val source=BitmapFactory.decodeByteArray(bytes,0,bytes.size,BitmapFactory.Options().apply{inSampleSize=sample})?:return null
            val b=Bitmap.createBitmap(96,96,Bitmap.Config.ARGB_8888);val c=Canvas(b)
            c.clipPath(Path().apply{addRoundRect(RectF(0f,0f,96f,96f),18f,18f,Path.Direction.CW)})
            val side=minOf(source.width,source.height);val x=(source.width-side)/2;val y=(source.height-side)/2
            c.drawBitmap(source,Rect(x,y,x+side,y+side),RectF(0f,0f,96f,96f),Paint(Paint.FILTER_BITMAP_FLAG));source.recycle();return b
        }
    }
}
