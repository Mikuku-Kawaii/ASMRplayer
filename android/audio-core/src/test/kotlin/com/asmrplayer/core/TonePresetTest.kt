package com.asmrplayer.core

import org.junit.Assert.*
import org.junit.Test
import kotlin.math.*

class TonePresetTest {
    @Test fun toneSelectionPreservesCalibrationLoudnessAndChannelSettings() {
        val current=DspConfig(bypass=true,preampDb=-4.5,bands=listOf(PeqBand(200.0,3.0)),balanceDb=-2.0,
            swap=true,limiter=false,ceilingDb=-4.0,leveler=true,normalize=true,targetLufs=-25.0,maxBoostDb=3.0)
        TonePreset.entries.forEach { preset ->
            val result=preset.applyTo(current)
            assertEquals(current,result.copy(bypass=current.bypass,highPass=current.highPass,cutoffHz=current.cutoffHz,
                deEsser=current.deEsser,air=current.air,warm=current.warm))
            assertTrue(preset.matches(result))
            assertEquals(1,TonePreset.entries.count{it.matches(result)})
        }
    }
    @Test fun warmShelfReducesHighFrequenciesAndKeepsStereoRatio() {
        fun rms(hz:Double,warm:Boolean):Double {
            val input=FloatArray(48000*2){i->(sin(2*PI*hz*(i/2)/48000)*if(i%2==0).1 else .05).toFloat()}
            val engine=DspEngine(48000,2,DspConfig(highPass=false,limiter=false,warm=warm))
            val output=engine.process(input)+engine.drain()
            assertEquals(input.size,output.size)
            for(i in 24000 until output.size step 2)assertEquals(output[i]*.5f,output[i+1],1e-7f)
            return sqrt(output.drop(24000).sumOf{it.toDouble()*it}/(output.size-24000))
        }
        assertTrue(rms(12000.0,true)<rms(12000.0,false)*.85)
        assertTrue(rms(200.0,true)>rms(200.0,false)*.99)
    }
    @Test fun allPresetsHotSwitchWithoutDroppingFramesOrBreakingPeakLimit() {
        val engine=DspEngine(48000,2)
        val output=mutableListOf<Float>()
        TonePreset.entries.forEach{preset->output+=engine.process(FloatArray(4096){sin(it*.13).toFloat()*2},preset).toList()}
        output+=engine.drain().toList()
        assertEquals(4096*TonePreset.entries.size,output.size)
        assertTrue(output.all{it.isFinite()&&abs(it)<=10.0.pow(-1.0/20)+1e-6})
    }
    private fun DspEngine.process(input:FloatArray,preset:TonePreset):FloatArray {update(preset.applyTo(DspConfig()));return process(input)}
}
