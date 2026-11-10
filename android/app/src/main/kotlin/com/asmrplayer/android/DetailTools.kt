package com.asmrplayer.android

import android.content.Context
import android.net.Uri
import com.asmrplayer.android.data.*
import com.asmrplayer.android.library.LibraryIndex
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*

data class DetailState(val workId:String?=null,val assets:List<WorkAsset> = emptyList(),val positions:List<PlaybackPosition> = emptyList(),val error:String?=null,val notice:String?=null,val busy:Boolean=false,val preview:String?=null,val remoteOrigin:String?=null,val remoteId:String?=null,val remoteAssets:List<RemoteAsset> = emptyList(),val ready:Boolean=false)
class DetailTools(context:Context,private val scope:CoroutineScope) {
    private val app = context.applicationContext
    private val backend=AndroidBackend.get(app)
    private val mutable=MutableStateFlow(DetailState())
    val state=mutable.asStateFlow()
    fun clearNotice() { mutable.update { it.copy(notice=null) } }
    private var reader:Job?=null
    private var action:Job?=null
    fun select(id:String?) {
        reader?.cancel();action?.cancel();mutable.value=DetailState(workId=id)
        if(id!=null)reader=scope.launch {
            try {
                val remote=backend.library.versions(id).first().firstNotNullOfOrNull{backend.database.library().remote(it.id)}
                mutable.update{it.copy(remoteOrigin=remote?.origin,remoteId=remote?.sourceWorkId)}
                combine(backend.library.assets(id),backend.library.positions,backend.online.assets(id)){a,p,r->Triple(a,p,r)}.collect{(a,p,r)->mutable.update{it.copy(assets=a,positions=p,remoteAssets=r,ready=true)}}
            }
            catch(e:CancellationException){throw e}catch(_:Exception){mutable.update{it.copy(error="详情资料无法读取，请返回重试。")}}
        }
    }
    private fun run(block:suspend ()->Unit) {
        if(mutable.value.busy)return
        mutable.update{it.copy(busy=true,error=null,notice=null)}
        action=scope.launch {try{block()}catch(e:CancellationException){throw e}catch(_:Exception){mutable.update{it.copy(notice="操作未完成，请检查文件授权或网络后重试。")}}finally{mutable.update{it.copy(busy=false)}}}
    }
    fun refresh(sourceId: String, origin: String, updated: () -> Unit = {}) = run {
        require(origin == backend.online.account.state.value.apiBase)
        val imported = backend.online.importWork(sourceId, false)
        com.asmrplayer.android.artwork.CoverArtwork.refresh(app, imported.workId)
        updated()
        mutable.update { it.copy(notice = "作品信息已更新。") }
    }
    fun download(ids:List<String>) {
        val workId=mutable.value.workId
        run {backend.downloads.enqueue(ids);workId?.let{backend.library.setMembership(it,true)};mutable.update{it.copy(notice="已提交下载任务。")}}
    }
    fun preview(asset:WorkAsset)=run {
        val source=SubtitleSource("preview","",asset.rootId,asset.relativePath,asset.uri,asset.relativePath.substringAfterLast('/'),asset.format,asset.available)
        val text=backend.subtitles.read(source).cues.joinToString("\n\n"){it.text}
        mutable.update{it.copy(preview=text)}
    }
    fun closePreview(){mutable.update{it.copy(preview=null)}}
    fun previewImage(asset:RemoteAsset,done:(String)->Unit)=run {done(backend.online.provider.resolve(asset).url)}
    fun downloadAsset(asset:RemoteAsset)=run {backend.downloads.enqueueAssets(listOf(asset.id));backend.library.setMembership(asset.workId,true)}
    fun bind(asset:WorkAsset,versionId:String,translation:Boolean)=run {
        val source=if(backend.database.library().remoteAsset(asset.id)!=null)backend.subtitles.attachDownloaded(versionId,asset.id)
            else backend.subtitles.attach(versionId,Uri.parse(asset.uri),asset.relativePath.substringAfterLast('/'),false)
        val pref=backend.database.library().subtitlePreference(versionId)?:SubtitlePreference(versionId)
        backend.subtitles.select(if(translation)pref.copy(translationId=source.id) else pref.copy(originalId=source.id))
        mutable.update{it.copy(notice="字幕已关联到所选音轨。")}
    }
}
