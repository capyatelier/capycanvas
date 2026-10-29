# Deferred windowed iPad drawing-tab drag

Deferred from 1.0.15 by the reviewer on October 28. Drawing tabs shipped in 1.0.14; no release regression is established. Do not merge this experiment into 1.0.15.

On the owned iPad Simulator in a windowed scene, run the existing `EditorLaunchTests/testNativeDrawingTabs` journey with its normal private app ID. It creates a second drawing and drags its header tab across the first tab. The whole window moves, and the order assertion at DrawingTabChecks.swift:37 fails. The journey remains unchanged.

The experiment registers UIWindowSceneDragInteraction on the equal-sized native horizontal strip host and makes window dragging wait for the app pan. It did not fix the reproduction: registration was confirmed on the real 773×36 native host, but the same journey failed. The temporary diagnostic logging is excluded here.

Reproduction evidence: artifacts/apple/release-1015/final-sim-tabs-light-13.log and final-sim-tabs-registration-13.log. Full-screen physical M4 passes both themes: final-m4-native-drawing-tabs-light-11.log and final-m4-native-drawing-tabs-dark-11.log.
