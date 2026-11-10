package com.asmrplayer.core

import kotlin.math.*

// RBJ Audio EQ Cookbook coefficients, double precision state per channel.
internal class Biquad(private val c: DoubleArray, channels: Int) {
    private val z1 = DoubleArray(channels)
    private val z2 = DoubleArray(channels)
    fun process(x: Double, ch: Int): Double {
        val y = c[0] * x + z1[ch]
        z1[ch] = c[1] * x - c[3] * y + z2[ch]
        z2[ch] = c[2] * x - c[4] * y
        return y
    }

    companion object {
        fun create(rate: Int, channels: Int, frequency: Double, gain: Double, q: Double, kind: FilterKind? = null): Biquad {
            require(frequency > 0 && frequency < rate * 0.5)
            val w = 2 * PI * frequency / rate
            val cos = cos(w)
            val alpha = sin(w) / (2 * q)
            val a = 10.0.pow(gain / 40)
            val beta = 2 * sqrt(a) * alpha
            val v = when (kind) {
                null -> doubleArrayOf((1+cos)/2, -(1+cos), (1+cos)/2, 1+alpha, -2*cos, 1-alpha)
                FilterKind.PEAK -> doubleArrayOf(1+alpha*a, -2*cos, 1-alpha*a, 1+alpha/a, -2*cos, 1-alpha/a)
                FilterKind.LOW_SHELF -> doubleArrayOf(a*((a+1)-(a-1)*cos+beta), 2*a*((a-1)-(a+1)*cos), a*((a+1)-(a-1)*cos-beta), (a+1)+(a-1)*cos+beta, -2*((a-1)+(a+1)*cos), (a+1)+(a-1)*cos-beta)
                FilterKind.HIGH_SHELF -> doubleArrayOf(a*((a+1)+(a-1)*cos+beta), -2*a*((a-1)+(a+1)*cos), a*((a+1)+(a-1)*cos-beta), (a+1)-(a-1)*cos+beta, 2*((a-1)-(a+1)*cos), (a+1)-(a-1)*cos-beta)
            }
            return Biquad(doubleArrayOf(v[0]/v[3], v[1]/v[3], v[2]/v[3], v[4]/v[3], v[5]/v[3]), channels)
        }
    }
}
