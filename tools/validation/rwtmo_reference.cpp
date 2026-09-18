// Independent numerical oracle: unmodified pinned Skia functions below.
// Copyright 2025 Google LLC. BSD-3-Clause; see THIRD_PARTY_NOTICES.md.
// Skia bc94efd2229aad1048edbf892a6b2e7db28b22c4, src/codec/SkHdrAgtm.cpp.
// Build with c++ -O2; stdout regenerates color/hdr/reference/skia-rwtmo.csv.
#include <vector>
#include <cmath>
#include <algorithm>
#include <cassert>
#include <cstdio>
#define SkASSERT assert
namespace SkNamedPrimaries { constexpr int kRec2020=9; }
struct AdaptiveGlobalToneMap {
 struct GainCurve { struct Point { float fX,fY,fM; }; std::vector<Point> fControlPoints; };
 struct ComponentMixing { float fMax=0; };
 struct ColorGainFunction { ComponentMixing fComponentMixing; GainCurve fGainCurve; };
 struct Alt { float fHdrHeadroom; ColorGainFunction fColorGainFunction; };
 struct HeadroomAdaptiveToneMap { int fGainApplicationSpacePrimaries; float fBaselineHdrHeadroom; std::vector<Alt> fAlternateImages; };
};
float EvaluateGainCurve(const AdaptiveGlobalToneMap::GainCurve& gainCurve, float x) {
    auto& cp = gainCurve.fControlPoints;
    size_t N = cp.size();

    // This implements that math in Formula 1 of SMPTE ST 2094-50.
    SkASSERT(N > 0 && N <= 32);

    // Handle points off of the left endpoint.
    size_t i = 0;
    if (x <= cp[i].fX) {
        return cp[i].fY;
    }

    // Handle points off of the right endpoint.
    size_t j = N - 1;
    if (x >= cp[j].fX) {
        return cp[j].fY + std::log2(cp[j].fX / x);
    }

    // Binary search for i, j bracket in which we find x.
    while (j - i > 1) {
        size_t m = (i + j) / 2;
        if (x < cp[m].fX) {
            j = m;
        } else {
            i = m;
        }
    }

    // Cache short names for the parameters for computing the cubic coefficients.
    const float x_i = cp[i].fX;
    const float y_i = cp[i].fY;
    const float x_j = cp[j].fX;
    const float y_j = cp[j].fY;
    const float h_i = x_j - x_i;
    const float mHat_i = cp[i].fM * h_i;
    const float mHat_j = cp[j].fM * h_i;

    // Handle intervals that are a point.
    if (h_i == 0.f) {
        return y_i;
    }

    // Compute the coefficients and evaluate the polynomial.
    const float c3 =  2.f * y_i + mHat_i - 2.f * y_j + mHat_j;
    const float c2 = -3.f * y_i + 3.f * y_j - 2.f * mHat_i - mHat_j;
    const float c1 = mHat_i;
    const float c0 = y_i;
    const float t = (x - x_i) / h_i;

    return ((c3*t + c2)*t + c1)*t + c0;
}
void PopulateUsingRwtmo(AdaptiveGlobalToneMap::HeadroomAdaptiveToneMap& hatm) {
    hatm.fGainApplicationSpacePrimaries = SkNamedPrimaries::kRec2020;

    if (hatm.fBaselineHdrHeadroom == 0.f) {
        hatm.fAlternateImages.clear();
        return;
    }

    // Set the two alternate image headrooms using Formula D.1 from ST 2094-50 candidate draft 2.
    hatm.fAlternateImages.resize(2);
    hatm.fAlternateImages[0].fHdrHeadroom = 0.f;
    hatm.fAlternateImages[1].fHdrHeadroom =
        std::log2(8.f / 3.f) * std::min(hatm.fBaselineHdrHeadroom / std::log2(1000/203.f), 1.f);

    for (size_t a = 0; a < hatm.fAlternateImages.size(); ++a) {
        auto& gain = hatm.fAlternateImages[a].fColorGainFunction;
        gain = AdaptiveGlobalToneMap::ColorGainFunction();

        // Use maxRGB for applying the curve.
        gain.fComponentMixing.fMax = 1.f;

        // Compute the image of white under the tone mapping from Formula D.2 from ST 2094-50
        // candidate draft 2.
        const float yWhite =
            (a == 1) ? 1.f
                     : 1.f - 0.5f * std::min(hatm.fBaselineHdrHeadroom / std::log2(1000/203.f), 1.f);

        // Compute the Bezier control points using Formula D.5 from ST 2094-50 candidate draft 2.
        const float kappa = 0.65f;
        const float xKnee = 1.f;
        const float yKnee = yWhite;
        const float xMax = std::exp2(hatm.fBaselineHdrHeadroom);
        const float yMax = std::exp2(hatm.fAlternateImages[a].fHdrHeadroom);
        const float xMid = (1.f - kappa) * xKnee + kappa * (xKnee * yMax / yKnee);
        const float yMid = (1.f - kappa) * yKnee + kappa * yMax;

        // Compute the cubic coefficients using Formula D.5 from ST 2094-50 candidate draft 2.
        const float xA = xKnee - 2.f * xMid + xMax;
        const float yA = yKnee - 2.f * yMid + yMax;
        const float xB = 2.f * xMid - 2.f * xKnee;
        const float yB = 2.f * yMid - 2.f * yKnee;
        const float xC = xKnee;
        const float yC = yKnee;

        auto& cubic = gain.fGainCurve;
        cubic.fControlPoints.resize(8);
        for (size_t c = 0; c < cubic.fControlPoints.size(); ++c) {
            // Compute the linear domain curve values using Formula D.4 from ST 2094-50 candidate
            // draft 2.
            const float t = c / (cubic.fControlPoints.size() - 1.f);
            const float x = xC + t * (xB + t * xA);
            const float y = yC + t * (yB + t * yA);
            const float m = (2.f * yA * t + yB) / (2.f * xA * t + xB);

            // Compute the log domain curve values using Formula D.3 from ST 2094-50 candidate
            // draft 2.
            cubic.fControlPoints[c].fX = x;
            cubic.fControlPoints[c].fY = std::log2(y / x);
            cubic.fControlPoints[c].fM = (x * m - y) / (std::log(2.f) * x * y);
        }
    }
}

int main() {
 for (float h: {0.25f,1.f,std::log2(1000.f/203.f),4.f,8.f,16.f}) {
  AdaptiveGlobalToneMap::HeadroomAdaptiveToneMap map;
  map.fBaselineHdrHeadroom=h; PopulateUsingRwtmo(map);
  for (int i=0;i<=256;i++) {
   float x=std::exp2(-8.f+i*(h+10.f)/256.f);
   float y=x*std::exp2(EvaluateGainCurve(map.fAlternateImages[0].fColorGainFunction.fGainCurve,x));
   std::printf("%.9g,%.9g,%.9g\n",h,x,y);
  }
 }
 for(float x:{0.f,.18f,1.f,2.f,4.f,16.f}) {
  AdaptiveGlobalToneMap::HeadroomAdaptiveToneMap map;
  map.fBaselineHdrHeadroom=std::log2(1000.f/203.f); PopulateUsingRwtmo(map);
  float y=x*std::exp2(EvaluateGainCurve(map.fAlternateImages[0].fColorGainFunction.fGainCurve,x));
  std::fprintf(stderr,"%.9g %.9g\n",x,y);
 }
}
