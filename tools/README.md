Excel -> DNP3 Tester JSON converter

Usage:

1. Install dependencies (recommended in a venv):

```bash
python3 -m pip install -r requirements.txt
```

2. Convert an Excel file (assumes columns like `type,index,name,description,unit,scale`):

```bash
python3 tools/excel_to_config.py "Palochi datalist.xlsx" -o frontend/default_config.json
```

3. Restart the DNP3 Tester server (or reload frontend). The frontend will auto-load `default_config.json` on page load, or use the "LOAD" button to load a local JSON.

Notes:
- Supported point type text values: `BinaryInput`, `BinaryOutput`, `AnalogInput`, `AnalogOutput`, `Counter` (case-insensitive). Some aliases like `BI/AO/BO/AI` are also accepted.
- If your Excel uses different column names, rename the columns to `type,index,name,...` or modify the script.
