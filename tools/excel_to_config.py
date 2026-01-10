#!/usr/bin/env python3
# Excel -> DNP3 Tester JSON config converter

import argparse
import json
import pandas as pd
from pathlib import Path

TYPE_MAP = {
    'binaryinput': 'binary_inputs',
    'binary input': 'binary_inputs',
    'bi': 'binary_inputs',
    'binaryoutput': 'binary_outputs',
    'binary output': 'binary_outputs',
    'bo': 'binary_outputs',
    'analoginput': 'analog_inputs',
    'analog input': 'analog_inputs',
    'ai': 'analog_inputs',
    'analogoutput': 'analog_outputs',
    'analog output': 'analog_outputs',
    'ao': 'analog_outputs',
    'counter': 'counters',
    'c': 'counters'
}

DEFAULT_COLUMNS = ['type','index','name','description','unit','scale']


def normalize_type(t):
    if pd.isna(t):
        return None
    s = str(t).strip().lower()
    return TYPE_MAP.get(s)


def load_excel(path):
    df = pd.read_excel(path, dtype=object)
    # normalize column names to lower
    df.columns = [c.strip().lower() for c in df.columns]
    return df


def row_to_obj(r, cols):
    obj = { 'index': int(r.get('index')) if pd.notna(r.get('index')) else None, 'name': str(r.get('name')) if pd.notna(r.get('name')) else '' }
    if 'description' in cols and pd.notna(r.get('description')):
        obj['description'] = str(r.get('description'))
    if 'unit' in cols and pd.notna(r.get('unit')):
        obj['unit'] = str(r.get('unit'))
    if 'scale' in cols and pd.notna(r.get('scale')):
        try:
            obj['scale'] = float(r.get('scale'))
        except Exception:
            pass
    return obj


def convert(excel_path, out_path):
    df = load_excel(excel_path)
    cols = set(df.columns)

    out = {
        'binary_inputs': [],
        'binary_outputs': [],
        'analog_inputs': [],
        'analog_outputs': [],
        'counters': []
    }

    required = False
    for idx, row in df.iterrows():
        # Try to find type column
        t = None
        for c in ['type','point_type','point type']:
            if c in df.columns:
                t = normalize_type(row.get(c))
                break
        if not t:
            # try infer from a column named like 'object' or first column
            first_col = df.columns[0]
            t = normalize_type(row.get(first_col))
            # if still not a type, skip
        if not t:
            continue
        obj = row_to_obj(row, cols)
        if obj['index'] is None:
            continue
        out[t].append(obj)
        required = True

    if not required:
        raise SystemExit('No valid rows found. Ensure a `type` (或 `point_type`) 列存在且包含点类型。')

    with open(out_path, 'w', encoding='utf-8') as f:
        json.dump(out, f, ensure_ascii=False, indent=2)

    counts = {k: len(v) for k, v in out.items()}
    print(f"Wrote {out_path} — counts: {counts}")


if __name__ == '__main__':
    p = argparse.ArgumentParser(description='Convert Palochi Excel datalist to DNP3 Tester JSON config')
    p.add_argument('excel', help='Path to Excel file (xlsx)')
    p.add_argument('-o','--output', default='frontend/default_config.json', help='Output JSON path')
    args = p.parse_args()
    excel = Path(args.excel)
    out = Path(args.output)
    if not excel.exists():
        raise SystemExit(f'Excel file not found: {excel}')
    convert(excel, out)
