package com.asmrplayer.android.widget

import android.content.Context
import java.io.File

internal data class WidgetGlassConfig(val mode:String="dark",val opacity:Float=.20f,val solid:Boolean=false,val revision:Long=0)

internal object WidgetGlassStore {
    private fun prefs(context:Context)=context.getSharedPreferences("widget_glass",Context.MODE_PRIVATE)
    fun read(context:Context,id:Int):WidgetGlassConfig {
        val p=prefs(context);val k="$id."
        return WidgetGlassConfig(p.getString(k+"mode","dark")?.takeIf{it in listOf("light","dark")}?:"dark",
            p.getFloat(k+"opacity",p.getFloat(k+"density",1f)*.20f).coerceIn(.08f,.36f),p.getBoolean(k+"solid",false),p.getLong(k+"revision",0))
    }
    @Synchronized fun save(context:Context,id:Int,config:WidgetGlassConfig) {
        removeLegacyImage(context,id)
        val k="$id."
        check(prefs(context).edit().putString(k+"mode",config.mode).putFloat(k+"opacity",config.opacity)
            .putBoolean(k+"solid",config.solid).putLong(k+"revision",System.currentTimeMillis()).commit()) {"保存失败，请重试"}
    }
    /** Remove only this widget's old app-private copy, never the user's original. */
    @Synchronized fun removeLegacyImage(context:Context,id:Int) {
        val p=prefs(context);val k="$id."
        val name=p.getString(k+"image",null)
        if(name?.matches(Regex("[a-f0-9-]+\\.png"))==true){
            val file=File(File(context.noBackupFilesDir,"widget-glass"),name)
            if(file.exists()&&!file.delete())return
        }
        if(listOf("image","x","y","w","h","density").any{p.contains(k+it)}){
            val opacity=read(context,id).opacity
            val edit=p.edit().putFloat(k+"opacity",opacity)
            listOf("image","x","y","w","h","density").forEach{edit.remove(k+it)}
            edit.commit()
        }
    }
    @Synchronized fun delete(context:Context,id:Int) {
        removeLegacyImage(context,id)
        val p=prefs(context);val edit=p.edit()
        p.all.keys.filter{it.startsWith("$id.")}.forEach{edit.remove(it)};edit.commit()
    }
}
