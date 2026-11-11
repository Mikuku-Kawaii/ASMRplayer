// Generated from lucide-react 0.468.0; ISC. See licenses/Lucide-LICENSE.txt. Do not hand-edit.
package com.asmrplayer.android.ui

import androidx.annotation.DrawableRes
import com.asmrplayer.android.R

enum class Glyph { Library, Discover, Heart, Queue, Menu, Search, Plus, Play, Pause, Next, Previous, Back, Eye, Download, Settings, Sliders, Audio, Stop, More, Folder, Collection, CollectionWide, TrackBookmark, Marker, Sleep, Sequence, Shuffle, Repeat, RepeatOne, Check, Close, Drag, Details, FileAudio, FileImage, ChevronDown, GroupAdd, Flame, Sales, NewArrival, Release, Discussion, History, ChevronRight, Star, Copy, Grid, List }

@DrawableRes
internal fun Glyph.lucideResource(filled: Boolean): Int = when (this) {
    Glyph.Library -> R.drawable.lucide_library_big
    Glyph.Discover -> R.drawable.lucide_compass
    Glyph.Heart -> if (filled) R.drawable.lucide_heart_filled else R.drawable.lucide_heart
    Glyph.Queue -> R.drawable.lucide_list_music
    Glyph.Menu -> R.drawable.lucide_menu
    Glyph.Search -> R.drawable.lucide_search
    Glyph.Plus -> R.drawable.lucide_plus
    Glyph.Play -> if (filled) R.drawable.lucide_play_filled else R.drawable.lucide_play
    Glyph.Pause -> if (filled) R.drawable.lucide_pause_filled else R.drawable.lucide_pause
    Glyph.Next -> R.drawable.lucide_skip_forward
    Glyph.Previous -> R.drawable.lucide_skip_back
    Glyph.Back -> R.drawable.lucide_chevron_left
    Glyph.Eye -> R.drawable.lucide_eye
    Glyph.Download -> R.drawable.lucide_download
    Glyph.Settings -> R.drawable.lucide_settings
    Glyph.Sliders -> R.drawable.lucide_sliders_horizontal
    Glyph.Audio -> R.drawable.lucide_audio_lines
    Glyph.Stop -> R.drawable.lucide_square
    Glyph.More -> R.drawable.lucide_ellipsis
    Glyph.Folder -> R.drawable.lucide_folder
    Glyph.Collection -> if (filled) R.drawable.lucide_bookmark_filled else R.drawable.lucide_bookmark
    Glyph.CollectionWide -> if (filled) R.drawable.lucide_bookmark_filled else R.drawable.lucide_bookmark
    Glyph.TrackBookmark -> if (filled) R.drawable.lucide_flag_filled else R.drawable.lucide_flag
    Glyph.Marker -> R.drawable.lucide_map_pin
    Glyph.Sleep -> if (filled) R.drawable.lucide_moon_filled else R.drawable.lucide_moon
    Glyph.Sequence -> R.drawable.lucide_list_end
    Glyph.Shuffle -> R.drawable.lucide_shuffle
    Glyph.Repeat -> R.drawable.lucide_repeat
    Glyph.RepeatOne -> R.drawable.lucide_repeat_1
    Glyph.Check -> R.drawable.lucide_check
    Glyph.Close -> R.drawable.lucide_x
    Glyph.Drag -> R.drawable.lucide_grip_vertical
    Glyph.Details -> R.drawable.lucide_file_text
    Glyph.FileAudio -> R.drawable.lucide_file_audio
    Glyph.FileImage -> R.drawable.lucide_file_image
    Glyph.ChevronDown -> R.drawable.lucide_chevron_down
    Glyph.GroupAdd -> R.drawable.lucide_square_plus
    Glyph.Flame -> R.drawable.lucide_flame
    Glyph.Sales -> R.drawable.lucide_trending_up
    Glyph.NewArrival -> R.drawable.lucide_sparkles
    Glyph.Release -> R.drawable.lucide_calendar_days
    Glyph.Discussion -> R.drawable.lucide_messages_square
    Glyph.History -> R.drawable.lucide_history
    Glyph.ChevronRight -> R.drawable.lucide_chevron_right
    Glyph.Star -> if (filled) R.drawable.lucide_star_filled else R.drawable.lucide_star
    Glyph.Copy -> R.drawable.lucide_copy
    Glyph.Grid -> R.drawable.lucide_layout_grid
    Glyph.List -> R.drawable.lucide_list
}
