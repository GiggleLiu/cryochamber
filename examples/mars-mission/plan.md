# Deep-Space Mission Planner

Plan and monitor a simulated Mars probe mission from launch to science completion.

## Goal

Manage a Mars probe mission spanning ~100 days of simulated mission time. Schedule trajectory corrections at optimal burn windows, sequence instrument calibrations, plan communication passes based on orbital geometry, and adapt the mission plan when anomalies occur.

## Tasks

1. Initialize mission: read parameters (delta-v budget, instrument manifest, science objectives). Compute initial trajectory. Schedule first trajectory correction maneuver (TCM-1).
2. Execute TCM burns at computed windows. Log delta-v expenditure. Recompute trajectory after each burn.
3. At T+30d: run full instrument checkout. Verify all sensors within calibration tolerance.
4. Handle anomalies: if an instrument fails or drifts, revise the science plan. Reprioritize objectives. Recalculate pointing schedules and data volume estimates.
5. At T+85d: pre-orbit-insertion systems check. Compute orbit insertion burn parameters.
6. At T+90d: execute orbit insertion. Begin science phase.
7. During science phase: collect data at observation windows determined by orbital position and lighting geometry. Each pass is aperiodic.
8. When all primary science objectives are met: write final mission report summarizing anomalies, adaptations, and results.

## Mission Parameters

- Delta-v budget: 2.1 km/s
- Orbit insertion estimate: 0.9 km/s
- Instruments: spectrometer, thermal imager, magnetometer, gravity gradiometer
- Primary science: atmospheric spectroscopy, thermal surface mapping
- Secondary science: gravity field characterization, magnetic field survey
- Comm relay: DSN passes scheduled by orbital geometry

## Anomaly Scenarios

The simulation may inject these events (check mission-events.json each session):
- Instrument failure (sensor offline permanently)
- Calibration drift (sensor needs recalibration at next thermal window)
- Solar storm (radiation damage to electronics)
- Missed burn window (must recompute trajectory with reduced delta-v)

## Notes

- Burn windows are aperiodic — determined by orbital mechanics, not calendar time. Wake times must match these windows precisely.
- A sensor failure changes everything downstream: science priorities, pointing schedules, data budgets, and communication planning.
- Delta-v is a finite, irreplaceable resource. Every decision must account for remaining budget.
- During cruise phases (no events), sleep intervals can be long (weeks). Near critical events (burns, orbit insertion), wake intervals should be short (days or hours).
