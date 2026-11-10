package com.asmrplayer.android.ui

import com.asmrplayer.android.artwork.*

import org.junit.Assert.*
import org.junit.Test

class OriginalCoverSelectionTest {
    @Test fun choosesLogoJacketOverBareIllustration() {
        val files=listOf("04_ジャケットイラスト/01_ジャケットイラスト.png","04_ジャケットイラスト/02_ジャケットイラスト（ロゴ）.png")
        assertEquals(files[1],files.maxByOrNull(::originalCoverPriority))
    }
    @Test fun excludesWallpapersPromoAndUnrelatedImages() {
        listOf("01_紹介画像.png","android_cover.png","jacket_wallpaper.png","cover_thumb.jpg","illustration.png","cover.txt").forEach{assertEquals(it,0,originalCoverPriority(it))}
    }
    @Test fun supportsMainAndLocalCoversWithoutPickingNoLogoFirst() {
        assertTrue(originalCoverPriority("封面.jpg")>0)
        assertTrue(originalCoverPriority("メインジャケット.png")>originalCoverPriority("ジャケット_ロゴなし.png"))
    }
    @Test fun supportsCommonWorkAndFolderNamesAcrossDifferentWorks() {
        listOf("RJ01445887_img_main.jpg","RJ01700554.png","folder.jpg","front.webp","表紙.png","ジャケット/01.png","covers/02_logo.jpg","パッケージ.png","特典/高画质封面图/package.jpg","イラスト/作品名_ロゴあり.png","作品名サムネ.png").forEach{assertTrue(it,originalCoverPriority(it)>0)}
        assertTrue(originalCoverPriority("package.jpg")>originalCoverPriority("package_off.jpg"))
        listOf("wallpapers/cover.png","紹介/ジャケット.png","images/01.png","covers/character.png","RJ01700554_img_smp1.jpg").forEach{assertEquals(it,0,originalCoverPriority(it))}
    }
    @Test fun normalCoverPrecedesThumbnailWhenMainCoverIsMissing() {
        val work=org.json.JSONObject().put("id",1700554).put("title","Test")
            .put("thumbnailCoverUrl","https://example.org/thumb.jpg").put("coverUrl","https://example.org/cover.jpg")
        assertEquals("https://example.org/cover.jpg",com.asmrplayer.android.online.AsmrJson.work(work).coverUrl)
        work.put("mainCoverUrl","https://example.org/main.jpg")
        assertEquals("https://example.org/main.jpg",com.asmrplayer.android.online.AsmrJson.work(work).coverUrl)
    }
}
