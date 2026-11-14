// Все частицы — инстансы одного квадрата. Форма рисуется процедурно в пиксельном шейдере (без текстур).
cbuffer C : register(b0) { float2 scr; float tm; float op; };

struct VO {
    float4 pos : SV_Position;
    float2 uv  : TEXCOORD0;
    float4 col : COLOR0;
    float4 k   : TEXCOORD1;   // rot, kind, p1, p2
};

VO vs(uint vid : SV_VertexID, float4 a : TEXCOORD0, float4 b : TEXCOORD1, float4 c : TEXCOORD2)
{
    float2 q = float2((vid & 1) ? 1.0 : -1.0, (vid & 2) ? 1.0 : -1.0);
    float s, co;
    sincos(b.x, s, co);
    float2 p = q * a.zw;
    p = float2(p.x * co - p.y * s, p.x * s + p.y * co);
    float2 px = a.xy + p;
    VO o;
    o.pos = float4(px.x / scr.x * 2.0 - 1.0, 1.0 - px.y / scr.y * 2.0, 0, 1);
    o.uv = q;
    o.col = c;
    o.k = b;
    return o;
}

// покрытие по знаковой дистанции (d < 0 внутри), сглаживание в 1 пиксель
float cover(float d) { float w = max(fwidth(d), 1e-4); return saturate(0.5 - d / w); }
float3 hue(float h) { return saturate(abs(frac(h + float3(0.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - 3.0) - 1.0); }

float4 ps(VO i) : SV_Target
{
    float2 uv = i.uv;
    float r = length(uv);
    int kind = (int)(i.k.y + 0.5);
    float3 c = i.col.rgb;
    float a = 0;
    float glow = 0;   // доля «аддитивности» (альфа меньше цвета)

    if (kind == 0) {            // мягкая точка (снег, боке). p1 — жёсткость края
        float h = i.k.z;
        a = 1.0 - smoothstep(h * 0.8, 1.0, r);
        a *= a;
    } else if (kind == 1) {     // снежинка
        float seg = 1.0471976;
        float ang = atan2(uv.y, uv.x) + seg * 10.5;
        float fa = abs(fmod(ang, seg) - seg * 0.5);
        float2 q = float2(cos(fa), sin(fa)) * r;
        float d = max(q.y - 0.075 * (1.0 - q.x * 0.6), q.x - 0.95);
        float2 bd = float2(0.5, 0.8660254);
        float2 b1 = q - float2(0.56, 0.0);
        d = min(d, length(b1 - bd * clamp(dot(b1, bd), 0.0, 0.30)) - 0.045);
        float2 b2 = q - float2(0.32, 0.0);
        d = min(d, length(b2 - bd * clamp(dot(b2, bd), 0.0, 0.20)) - 0.045);
        d = min(d, r - 0.16);
        a = max(cover(d), 0.22 * (1.0 - smoothstep(0.0, 1.0, r)));
    } else if (kind == 2) {     // лепесток сакуры (основание внизу, выемка сверху)
        float s = uv.y * 0.5 + 0.5;
        float w = 0.92 * pow(saturate(sin(3.14159 * saturate(s * 0.92 + 0.04))), 0.55) * (1.0 - 0.45 * s * s);
        float d = abs(uv.x) - w;
        d = max(d, (-0.62 - uv.y) * 0.9 - abs(uv.x) * 0.9 + 0.12);
        a = cover(d);
        c *= 1.12 - 0.3 * s;
        c = lerp(c, float3(1.0, 0.95, 0.97), 0.35 * (1.0 - s) * (1.0 - abs(uv.x)));
        c *= 1.0 - 0.18 * exp(-abs(uv.x) * 26.0) * s;
        if (i.k.z < 0) c *= 0.85;
    } else if (kind == 3) {     // лист (кончик сверху, черешок снизу)
        float s = uv.y * 0.5 + 0.5;
        float sb = saturate(s / 0.86);
        float w = 0.64 * pow(saturate(sin(3.14159 * sb)), 0.8) * (1.0 + 0.08 * sin(s * 44.0));
        float d = abs(uv.x) - w;
        d = max(d, s - 0.88);
        float stem = max(abs(uv.x) - 0.035, abs(s - 0.92) - 0.08);
        d = min(d, stem);
        a = cover(d);
        float vein = exp(-abs(uv.x) * 40.0) + 0.6 * exp(-abs(frac(s * 6.0 + abs(uv.x) * 3.0) - 0.5) * 18.0) * step(abs(uv.x), w * 0.85);
        c *= 0.82 + 0.3 * (1.0 - s) - 0.25 * saturate(vein);
        if (i.k.z < 0) c *= 0.8;
    } else if (kind == 4) {     // капля дождя (голова внизу uv.y = 1)
        a = pow(saturate(uv.y * 0.5 + 0.5), 1.5) * saturate(1.0 - abs(uv.x));
    } else if (kind == 5) {     // светлячок
        float core = exp(-r * r * 55.0);
        float halo = exp(-r * r * 5.5);
        a = core * 0.95 + halo * 0.35;
        glow = 0.7;
        c = lerp(c, float3(1, 1, 0.9), core * 0.7);
    } else if (kind == 6) {     // конфетти
        a = cover(max(abs(uv.x), abs(uv.y)) - 0.97);
        c *= 0.7 + 0.35 * abs(i.k.z);
    } else if (kind == 7) {     // мыльный пузырь
        float rim = exp(-pow((r - 0.86) / 0.08, 2.0));
        float fill = 0.07 * smoothstep(1.0, 0.2, r);
        float hl = exp(-dot(uv - float2(-0.38, -0.42), uv - float2(-0.38, -0.42)) * 45.0);
        float3 ir = hue(atan2(uv.y, uv.x) / 6.2831853 + i.k.w + tm * 0.07);
        c = lerp(float3(1, 1, 1), ir, 0.6) * c;
        a = (rim * 0.75 + fill) * step(r, 1.0);
        c = lerp(c, float3(1, 1, 1), saturate(hl * 2.0));
        a = saturate(a + hl * 0.85);
    } else if (kind == 8) {     // искра (шлейф курсора)
        float s1 = exp(-abs(uv.x) * 11.0) * exp(-abs(uv.y) * 2.3);
        float s2 = exp(-abs(uv.y) * 11.0) * exp(-abs(uv.x) * 2.3);
        float core = exp(-r * r * 16.0);
        a = saturate(max(s1, s2) * 0.9 + core);
        glow = 0.5;
    } else if (kind == 9) {     // туман у низа экрана
        a = pow(saturate(uv.y * 0.5 + 0.5), 1.7) * (0.85 + 0.15 * sin(uv.x * 2.7 + tm * 0.15));
    } else if (kind == 10) {    // вспышка молнии
        a = 1.0;
        glow = 0.6;
    } else if (kind == 11) {    // брызги / лопнувший пузырь — кольцо
        a = exp(-pow((r - 0.8) / 0.11, 2.0)) * step(r, 1.0);
    } else if (kind == 12) {    // бабочка (вид сверху), голова вверху
        float2 q = float2(abs(uv.x), uv.y);
        // верхнее крыло — широкое, нижнее — меньше и округлее
        float up = length((q - float2(0.50, -0.22)) / float2(0.50, 0.46)) - 1.0;
        float lo = length((q - float2(0.34, 0.40)) / float2(0.34, 0.36)) - 1.0;
        float wing = min(up, lo);
        float bodyd = length(float2(uv.x / 0.09, (uv.y - 0.02) / 0.70)) - 1.0;
        float wa = cover(wing * 0.45);
        float ba = cover(bodyd * 0.09);
        a = max(wa, ba);
        // тёмная кайма и прожилки, светлые пятна по краю
        float edge = smoothstep(-0.30, -0.08, wing);
        float vein = exp(-abs(frac(atan2(q.y + 0.05, q.x) * 1.6) - 0.5) * 30.0) * 0.5 * step(wing, -0.1);
        float spot = smoothstep(0.07, 0.03, length(q - float2(0.86, -0.38))) + smoothstep(0.06, 0.025, length(q - float2(0.72, -0.62))) + smoothstep(0.05, 0.02, length(q - float2(0.55, 0.62)));
        c = lerp(c * 1.1, c * 0.5, saturate(length(q - float2(0.12, 0.05)) * 0.6));
        c = lerp(c, float3(0.08, 0.06, 0.05), saturate(edge * 0.9 + vein));
        c = lerp(c, float3(1, 1, 1), saturate(spot) * edge);
        c = lerp(c, float3(0.1, 0.08, 0.07), ba);
        c *= 0.85 + 0.2 * abs(i.k.z);
    } else if (kind == 13) {    // пушинка одуванчика: семечко снизу, ножка, зонтик из ворсинок
        float2 pv = uv - float2(0.0, -0.15);
        float rr = length(pv);
        float ang = atan2(pv.x, -pv.y);
        float fil = pow(abs(cos(ang * 8.0)), 14.0) * smoothstep(1.55, 1.2, abs(ang)) * smoothstep(0.92, 0.55, rr) * smoothstep(0.03, 0.12, rr);
        float tips = exp(-pow((rr - 0.82) / 0.12, 2.0)) * smoothstep(1.55, 1.2, abs(ang)) * 0.45;
        float core = exp(-rr * rr * 60.0) * 0.8;
        fil = max(fil, core);
        float stem = cover(max(abs(uv.x) - 0.025, abs(uv.y - 0.33) - 0.48) * 0.5);
        float seed = cover((length(float2(uv.x / 0.07, (uv.y - 0.86) / 0.13)) - 1.0) * 0.07);
        a = saturate(fil * 0.85 + tips + stem * 0.8 + seed);
        c = lerp(c, float3(0.55, 0.45, 0.3), seed);
        glow = 0.15;
    }

    a *= i.col.a * op;
    return float4(c * a, a * (1.0 - glow));
}
