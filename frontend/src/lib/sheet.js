import * as XLSX from 'xlsx'

function usedWidth(row) {
  let n = row.length
  while (n > 0 && String(row[n - 1] ?? '').trim() === '') n--
  return n
}

export function sheetFromWb(wb, name) {
  const ws = wb.Sheets[name]
  const rows = XLSX.utils.sheet_to_json(ws, { header: 1, defval: '', blankrows: true, raw: false })
  const w = Math.max(1, ...rows.map(usedWidth))
  const norm = rows.map((r) => Array.from({ length: w }, (_, i) => (r[i] == null ? '' : String(r[i]))))
  return { names: wb.SheetNames, active: name, rows: norm.length ? norm : [['']] }
}
