"""Per-capture regression snapshots for Smooth Motion.

Version 2 evaluates each recording independently, without narrowing the common
future interval to an obsolete comparison model.
"""
VERSION = 2


def regression_snapshot(result):
    rows = {}

    def add(name, metric, thresholds=()):
        row = dict(mean=metric['mean'], eligible_seconds=metric['eligible_seconds'])
        for t in thresholds:
            row[f'ge{t}_seconds'] = metric['thresholds'][str(float(t))]['seconds']
        rows[name] = row

    for key in ['tip', 'body']:
        add(f'ordinary correction {key}', result['correction_smoothness']['ordinary'][key], [.5, 1, 2, 4, 8, 16])
        add(f'oscillation {key}', result['oscillation'][key], [.5, 1, 2, 4, 8, 16])
        add(f'straight retreat {key}', result['retraction']['straightish'][f'retreat_{key}'], [.5, 1, 2, 4, 8, 16])
    for key in ['off_path_tip', 'flash_tip', 'flash_body']:
        add(f'braking {key}', result['braking'][key], [1, 8, 16])
    for group in ['all', 'slow motion', 'medium motion', 'slow/medium steady', 'slow/medium changing', 'fast predictable']:
        for key in ['gap_0', 'gap_8', 'gap_16', 'gap_24']:
            add(f'{group} {key}', result['balance'][group][key])
        for key in ['off_path_tip', 'off_path_body']:
            add(f'{group} {key}', result['balance'][group][key], [2, 8, 16, 32, 64])
    return dict(version=VERSION, metrics=rows)


def check_snapshot(expected, actual):
    """5% regression allowance plus .025 s sparse exposure / .01 px mean.

    Eligibility must not disappear to make a score look better. These are
    engineering budgets, not perceptual thresholds or statistical confidence.
    """
    if expected.get('version') != VERSION or actual.get('version') != VERSION:
        return ['unsupported correction-stability baseline version']
    a, b = expected['metrics'], actual['metrics']
    if a.keys() != b.keys():
        return ['correction-stability baseline metrics differ']
    failures = []
    for name, row in a.items():
        if row.keys() != b[name].keys():
            failures.append(f'{name}: baseline fields differ')
            continue
        for key, before in row.items():
            after = b[name][key]
            if key == 'eligible_seconds':
                if after < before * .99 - .025:
                    failures.append(f'{name}: lost scoring eligibility')
            elif after > before * 1.05 + (.025 if key.endswith('_seconds') else .01):
                failures.append(f'{name} {key}: {before:.6f} -> {after:.6f}')
    return failures
