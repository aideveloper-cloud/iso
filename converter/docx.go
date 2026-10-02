package main

// docx -> HTML ที่คงรูปแบบเอกสารเดิมให้มากที่สุด (ตัวอักษร ตาราง สี เส้นขอบ รูป หัว/ท้ายกระดาษ เลขรายการ)
// ทุกย่อหน้า/แถว/run ที่แก้ได้จะมี data-p / data-tr / data-r เพื่อให้ฝั่งเว็บ "แก้ไฟล์ .docx เดิมในที่เดิม"
// (lib/docxpatch.js) แทนการสร้างไฟล์ใหม่ — ต้องเรียงเลข/จัดกลุ่ม run ให้ตรงกับ docxpatch.js เสมอ

import (
	"archive/zip"
	"bytes"
	"encoding/base64"
	"encoding/xml"
	"fmt"
	"io"
	"math"
	"path"
	"strconv"
	"strings"
)

const nsW = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"

// ---------- XML tree ----------
type node struct {
	space, name string
	attr        map[string]string
	kids        []*node
	text        string
	idx         int
	parent      *node
}

func parseTree(data []byte) *node {
	dec := xml.NewDecoder(bytes.NewReader(data))
	dec.Strict = false
	root := &node{name: "#root"}
	cur := root
	for {
		tok, err := dec.Token()
		if err != nil {
			break
		}
		switch t := tok.(type) {
		case xml.StartElement:
			n := &node{space: t.Name.Space, name: t.Name.Local, parent: cur, attr: map[string]string{}}
			for _, a := range t.Attr {
				n.attr[a.Name.Local] = a.Value
			}
			cur.kids = append(cur.kids, n)
			cur = n
		case xml.EndElement:
			if cur.parent != nil {
				cur = cur.parent
			}
		case xml.CharData:
			cur.text += string(t)
		}
	}
	if len(root.kids) > 0 {
		root.kids[0].parent = nil
		return root.kids[0]
	}
	return root
}

func (n *node) w(name string) *node {
	if n == nil {
		return nil
	}
	for _, k := range n.kids {
		if k.space == nsW && k.name == name {
			return k
		}
	}
	return nil
}

func (n *node) is(name string) bool { return n != nil && n.space == nsW && n.name == name }
func (n *node) val() string {
	if n == nil {
		return ""
	}
	return n.attr["val"]
}

// เลขลำดับ w:p และ w:tr แบบ pre-order ทั้งไฟล์ (ต้องตรงกับ getElementsByTagNameNS ฝั่ง JS)
func numberTree(root *node) {
	p, t := 0, 0
	var walk func(n *node)
	walk = func(n *node) {
		if n.space == nsW {
			switch n.name {
			case "p":
				n.idx = p
				p++
			case "tr":
				n.idx = t
				t++
			}
		}
		for _, k := range n.kids {
			walk(k)
		}
	}
	walk(root)
}

func atoi(s string) int {
	v, _ := strconv.Atoi(strings.TrimSpace(s))
	return v
}

// ---------- ค่าคุณสมบัติ ----------
type opt struct {
	v  int
	ok bool
}

func (o opt) or(b opt) opt {
	if b.ok {
		return b
	}
	return o
}

func optAttr(n *node, key string) opt {
	if n == nil {
		return opt{}
	}
	s, has := n.attr[key]
	if !has || s == "" {
		return opt{}
	}
	f, err := strconv.ParseFloat(s, 64)
	if err != nil {
		return opt{}
	}
	return opt{int(f), true}
}

type rp struct {
	b, bcs, i, ics, u, strike, caps int // 0 ไม่กำหนด, 1 เปิด, -1 ปิด
	sz, szcs                        int
	color, hl, fontA, fontCS, vert  string
}

func onoff(n *node) int {
	if n == nil {
		return 0
	}
	v := strings.ToLower(n.attr["val"])
	if v == "0" || v == "false" || v == "off" {
		return -1
	}
	return 1
}

var hlMap = map[string]string{
	"yellow": "#ffff00", "green": "#00ff00", "cyan": "#00ffff", "magenta": "#ff00ff", "blue": "#0000ff",
	"red": "#ff0000", "darkBlue": "#000080", "darkCyan": "#008080", "darkGreen": "#008000",
	"darkMagenta": "#800080", "darkRed": "#800000", "darkYellow": "#808000", "darkGray": "#808080",
	"lightGray": "#c0c0c0", "black": "#000000", "white": "#ffffff",
}

func parseRPr(n *node) rp {
	var r rp
	if n == nil {
		return r
	}
	for _, k := range n.kids {
		if k.space != nsW {
			continue
		}
		switch k.name {
		case "b":
			r.b = onoff(k)
		case "bCs":
			r.bcs = onoff(k)
		case "i":
			r.i = onoff(k)
		case "iCs":
			r.ics = onoff(k)
		case "u":
			if k.val() == "none" {
				r.u = -1
			} else {
				r.u = 1
			}
		case "strike", "dstrike":
			r.strike = onoff(k)
		case "caps", "smallCaps":
			r.caps = onoff(k)
		case "sz":
			r.sz = atoi(k.val())
		case "szCs":
			r.szcs = atoi(k.val())
		case "color":
			if v := k.val(); v != "" && v != "auto" {
				r.color = "#" + v
			}
		case "highlight":
			r.hl = hlMap[k.val()]
		case "rFonts":
			r.fontA = firstNonEmpty(k.attr["ascii"], k.attr["hAnsi"], k.attr["eastAsia"])
			r.fontCS = k.attr["cs"]
		case "vertAlign":
			r.vert = k.val()
		}
	}
	return r
}

func firstNonEmpty(ss ...string) string {
	for _, s := range ss {
		if s != "" {
			return s
		}
	}
	return ""
}

func (a rp) over(b rp) rp {
	pick := func(x, y int) int {
		if y != 0 {
			return y
		}
		return x
	}
	ps := func(x, y string) string {
		if y != "" {
			return y
		}
		return x
	}
	return rp{
		b: pick(a.b, b.b), bcs: pick(a.bcs, b.bcs), i: pick(a.i, b.i), ics: pick(a.ics, b.ics),
		u: pick(a.u, b.u), strike: pick(a.strike, b.strike), caps: pick(a.caps, b.caps),
		sz: pick(a.sz, b.sz), szcs: pick(a.szcs, b.szcs),
		color: ps(a.color, b.color), hl: ps(a.hl, b.hl), fontA: ps(a.fontA, b.fontA),
		fontCS: ps(a.fontCS, b.fontCS), vert: ps(a.vert, b.vert),
	}
}

type border struct {
	val, color string
	sz         int
	set        bool
}

func parseBorder(n *node) border {
	if n == nil {
		return border{}
	}
	return border{val: n.attr["val"], color: n.attr["color"], sz: atoi(n.attr["sz"]), set: true}
}

// ลำดับ: top, left, bottom, right, insideH, insideV
var sideNames = [6]string{"top", "left", "bottom", "right", "insideH", "insideV"}

func parseBorders(n *node) [6]border {
	var out [6]border
	if n == nil {
		return out
	}
	for i, s := range sideNames {
		b := n.w(s)
		if b == nil {
			switch s {
			case "left":
				b = n.w("start")
			case "right":
				b = n.w("end")
			}
		}
		out[i] = parseBorder(b)
	}
	return out
}

func (b border) css(side string) string {
	if !b.set {
		return ""
	}
	switch b.val {
	case "", "nil", "none":
		return "border-" + side + ":none;"
	}
	w := int(math.Round(float64(b.sz) / 8 * 1.3333))
	if w < 1 {
		w = 1
	}
	st := "solid"
	switch {
	case b.val == "double":
		st = "double"
		if w < 3 {
			w = 3
		}
	case strings.Contains(b.val, "dash"):
		st = "dashed"
	case strings.Contains(b.val, "dot"):
		st = "dotted"
	}
	col := "#000"
	if b.color != "" && b.color != "auto" {
		col = "#" + b.color
	}
	return fmt.Sprintf("border-%s:%dpx %s %s;", side, w, st, col)
}

type pp struct {
	jc                       string
	left, right, first, hang opt
	before, after, line      opt
	lineRule                 string
	shd                      string
	numID, ilvl              opt
	numIDStr                 string
	bdr                      [4]border // top,left,bottom,right
	hasBdr                   bool
}

func parsePPr(n *node) pp {
	var p pp
	if n == nil {
		return p
	}
	for _, k := range n.kids {
		if k.space != nsW {
			continue
		}
		switch k.name {
		case "jc":
			p.jc = k.val()
		case "ind":
			l := optAttr(k, "left")
			if !l.ok {
				l = optAttr(k, "start")
			}
			r := optAttr(k, "right")
			if !r.ok {
				r = optAttr(k, "end")
			}
			p.left, p.right = l, r
			p.first = optAttr(k, "firstLine")
			p.hang = optAttr(k, "hanging")
		case "spacing":
			p.before, p.after, p.line = optAttr(k, "before"), optAttr(k, "after"), optAttr(k, "line")
			p.lineRule = k.attr["lineRule"]
		case "shd":
			if f := k.attr["fill"]; f != "" && f != "auto" {
				p.shd = "#" + f
			}
		case "numPr":
			if id := k.w("numId"); id != nil {
				p.numIDStr = id.val()
				p.numID = opt{atoi(id.val()), true}
			}
			if l := k.w("ilvl"); l != nil {
				p.ilvl = opt{atoi(l.val()), true}
			}
		case "pBdr":
			b := parseBorders(k)
			p.bdr = [4]border{b[0], b[1], b[2], b[3]}
			p.hasBdr = true
		}
	}
	return p
}

func (a pp) over(b pp) pp {
	if b.jc != "" {
		a.jc = b.jc
	}
	a.left, a.right = a.left.or(b.left), a.right.or(b.right)
	if b.first.ok || b.hang.ok {
		a.first, a.hang = b.first, b.hang
	}
	a.before, a.after, a.line = a.before.or(b.before), a.after.or(b.after), a.line.or(b.line)
	if b.lineRule != "" {
		a.lineRule = b.lineRule
	}
	if b.shd != "" {
		a.shd = b.shd
	}
	if b.numID.ok {
		a.numID, a.numIDStr = b.numID, b.numIDStr
	}
	a.ilvl = a.ilvl.or(b.ilvl)
	if b.hasBdr {
		a.bdr, a.hasBdr = b.bdr, true
	}
	return a
}

// ---------- styles ----------
type style struct {
	typ, basedOn string
	rpr          rp
	ppr          pp
	tbl          [6]border
	hasTbl       bool
}

type styleSet struct {
	m          map[string]*style
	defR       rp
	defP       pp
	defParaID  string
	defTableID string
}

func loadStyles(data []byte) *styleSet {
	s := &styleSet{m: map[string]*style{}}
	if data == nil {
		return s
	}
	root := parseTree(data)
	if dd := root.w("docDefaults"); dd != nil {
		if x := dd.w("rPrDefault"); x != nil {
			s.defR = parseRPr(x.w("rPr"))
		}
		if x := dd.w("pPrDefault"); x != nil {
			s.defP = parsePPr(x.w("pPr"))
		}
	}
	for _, k := range root.kids {
		if !k.is("style") {
			continue
		}
		id := k.attr["styleId"]
		st := &style{typ: k.attr["type"], basedOn: k.w("basedOn").val()}
		st.rpr = parseRPr(k.w("rPr"))
		st.ppr = parsePPr(k.w("pPr"))
		if tp := k.w("tblPr"); tp != nil {
			if tb := tp.w("tblBorders"); tb != nil {
				st.tbl, st.hasTbl = parseBorders(tb), true
			}
		}
		s.m[id] = st
		if k.attr["default"] == "1" {
			switch st.typ {
			case "paragraph":
				s.defParaID = id
			case "table":
				s.defTableID = id
			}
		}
	}
	return s
}

func (s *styleSet) rpr(id string, depth int) rp {
	st := s.m[id]
	if st == nil || depth > 10 {
		return rp{}
	}
	return s.rpr(st.basedOn, depth+1).over(st.rpr)
}

func (s *styleSet) ppr(id string, depth int) pp {
	st := s.m[id]
	if st == nil || depth > 10 {
		return pp{}
	}
	return s.ppr(st.basedOn, depth+1).over(st.ppr)
}

func (s *styleSet) tblBorders(id string, depth int) ([6]border, bool) {
	st := s.m[id]
	if st == nil || depth > 10 {
		return [6]border{}, false
	}
	if st.hasTbl {
		return st.tbl, true
	}
	return s.tblBorders(st.basedOn, depth+1)
}

// ---------- numbering ----------
type lvl struct {
	fmt, text string
	start     int
	left      opt
	hang      opt
}

type numbering struct {
	abs map[string]map[int]*lvl
	num map[string]string
}

func loadNumbering(data []byte) *numbering {
	n := &numbering{abs: map[string]map[int]*lvl{}, num: map[string]string{}}
	if data == nil {
		return n
	}
	root := parseTree(data)
	for _, k := range root.kids {
		switch {
		case k.is("abstractNum"):
			m := map[int]*lvl{}
			for _, l := range k.kids {
				if !l.is("lvl") {
					continue
				}
				ind := parsePPr(l.w("pPr"))
				lv := &lvl{fmt: l.w("numFmt").val(), text: l.w("lvlText").val(), start: 1, left: ind.left, hang: ind.hang}
				if s := l.w("start"); s != nil {
					lv.start = atoi(s.val())
				}
				m[atoi(l.attr["ilvl"])] = lv
			}
			n.abs[k.attr["abstractNumId"]] = m
		case k.is("num"):
			n.num[k.attr["numId"]] = k.w("abstractNumId").val()
		}
	}
	return n
}

func (n *numbering) level(numID string, ilvl int) *lvl {
	return n.abs[n.num[numID]][ilvl]
}

const thaiLetters = "กขฃคฅฆงจฉชซฌญฎฏฐฑฒณดตถทธนบปผฝพฟภมยรลวศษสหฬอฮ"

func fmtNum(f string, n int) string {
	switch f {
	case "lowerLetter", "upperLetter":
		if n < 1 {
			n = 1
		}
		s := string(rune('a' + (n-1)%26))
		s = strings.Repeat(s, (n-1)/26+1)
		if f == "upperLetter" {
			return strings.ToUpper(s)
		}
		return s
	case "lowerRoman", "upperRoman":
		vals := []int{1000, 900, 500, 400, 100, 90, 50, 40, 10, 9, 5, 4, 1}
		syms := []string{"m", "cm", "d", "cd", "c", "xc", "l", "xl", "x", "ix", "v", "iv", "i"}
		var b strings.Builder
		for i, v := range vals {
			for n >= v {
				b.WriteString(syms[i])
				n -= v
			}
		}
		if f == "upperRoman" {
			return strings.ToUpper(b.String())
		}
		return b.String()
	case "thaiNumbers":
		r := []rune(strconv.Itoa(n))
		for i, c := range r {
			r[i] = '๐' + (c - '0')
		}
		return string(r)
	case "thaiLetters":
		rs := []rune(thaiLetters)
		if n < 1 {
			n = 1
		}
		return string(rs[(n-1)%len(rs)])
	case "decimalZero":
		return fmt.Sprintf("%02d", n)
	}
	return strconv.Itoa(n)
}

func bulletChar(s string) string {
	r := []rune(s)
	if len(r) == 0 {
		return "•"
	}
	if r[0] >= 0xF000 {
		if r[0] == 0xF0A7 || r[0] == 0xF0A8 {
			return "▪"
		}
		return "•"
	}
	if s == "o" {
		return "◦"
	}
	return s
}

// ---------- converter ----------
type conv struct {
	files    map[string][]byte
	st       *styleSet
	num      *numbering
	part     string
	rels     map[string]string
	counters map[string]*[9]int
	b        strings.Builder
}

func hasThai(s string) bool {
	for _, r := range s {
		if r >= 0x0E00 && r <= 0x0E7F {
			return true
		}
	}
	return false
}

var htmlEscaper = strings.NewReplacer("&", "&amp;", "<", "&lt;", ">", "&gt;", `"`, "&quot;")

func fontCSS(name string) string {
	name = strings.NewReplacer("'", "", `"`, "", ";", "", "<", "", ">", "").Replace(name)
	if name == "" {
		return "'TH Sarabun New','Sarabun',Tahoma,sans-serif"
	}
	return "'" + name + "','TH Sarabun New','Sarabun',Tahoma,sans-serif"
}

func loadRels(files map[string][]byte, part string) map[string]string {
	out := map[string]string{}
	data := files["word/_rels/"+part+".xml.rels"]
	if data == nil {
		return out
	}
	root := parseTree(data)
	for _, k := range root.kids {
		if k.name == "Relationship" && k.attr["TargetMode"] != "External" {
			t := k.attr["Target"]
			if strings.HasPrefix(t, "/") {
				t = strings.TrimPrefix(t, "/")
			} else {
				t = path.Clean(path.Join("word", t))
			}
			out[k.attr["Id"]] = t
		}
	}
	return out
}

func docxToHTML(p string) (string, error) {
	zr, err := zip.OpenReader(p)
	if err != nil {
		return "", fmt.Errorf("เปิดไฟล์ไม่ได้: %w", err)
	}
	defer zr.Close()
	files := map[string][]byte{}
	for _, f := range zr.File {
		n := f.Name
		if n == "word/document.xml" || n == "word/styles.xml" || n == "word/numbering.xml" ||
			strings.HasSuffix(n, ".rels") || strings.HasPrefix(n, "word/header") || strings.HasPrefix(n, "word/footer") ||
			strings.HasPrefix(n, "word/media/") {
			if f.UncompressedSize64 > 40<<20 {
				continue
			}
			rc, e := f.Open()
			if e != nil {
				continue
			}
			files[n], _ = io.ReadAll(rc)
			rc.Close()
		}
	}
	if files["word/document.xml"] == nil {
		return "", fmt.Errorf("ไม่พบ word/document.xml")
	}
	return renderDocx(files), nil
}

func renderDocx(files map[string][]byte) string {
	c := &conv{files: files, st: loadStyles(files["word/styles.xml"]), num: loadNumbering(files["word/numbering.xml"]), counters: map[string]*[9]int{}}
	doc := parseTree(files["word/document.xml"])
	numberTree(doc)
	body := doc.w("body")
	if body == nil {
		return ""
	}
	docRels := loadRels(files, "document")

	// ขนาดหน้าและขอบกระดาษจาก sectPr ท้ายเอกสาร
	pageW, mT, mR, mB, mL := 11906, 1440, 1440, 1440, 1440
	var sect *node
	for i := len(body.kids) - 1; i >= 0; i-- {
		if body.kids[i].is("sectPr") {
			sect = body.kids[i]
			break
		}
	}
	if sect != nil {
		if s := sect.w("pgSz"); s != nil && atoi(s.attr["w"]) > 0 {
			pageW = atoi(s.attr["w"])
		}
		if m := sect.w("pgMar"); m != nil {
			mT, mR, mB, mL = atoi(m.attr["top"]), atoi(m.attr["right"]), atoi(m.attr["bottom"]), atoi(m.attr["left"])
		}
	}
	hdr := c.partHTML(sect, "headerReference", docRels)
	ftr := c.partHTML(sect, "footerReference", docRels)

	c.part, c.rels = "document", docRels
	var out strings.Builder
	fmt.Fprintf(&out, `<div class="docx-page" style="width:%dpx;padding:%dpx %dpx %dpx %dpx">`, pageW/15, mT/15, mR/15, mB/15, mL/15)
	out.WriteString(hdr)
	c.b.Reset()
	c.blocks(body)
	out.WriteString(`<div class="docx-body">` + c.b.String() + `</div>`)
	out.WriteString(ftr)
	out.WriteString(`</div>`)
	return out.String()
}

func (c *conv) partHTML(sect *node, refName string, docRels map[string]string) string {
	if sect == nil {
		return ""
	}
	var target string
	for _, k := range sect.kids {
		if k.is(refName) && (k.attr["type"] == "default" || k.attr["type"] == "") {
			target = docRels[k.attr["id"]]
			break
		}
	}
	data := c.files[target]
	if target == "" || data == nil {
		return ""
	}
	root := parseTree(data)
	numberTree(root)
	name := strings.TrimSuffix(path.Base(target), ".xml")
	c.part, c.rels = name, loadRels(c.files, name)
	c.b.Reset()
	c.blocks(root)
	cls := "docx-hdr"
	if refName == "footerReference" {
		cls = "docx-ftr"
	}
	return `<div class="` + cls + `">` + c.b.String() + `</div>`
}

func (c *conv) blocks(parent *node) {
	for _, k := range parent.kids {
		if k.space != nsW {
			continue
		}
		switch k.name {
		case "p":
			c.para(k)
		case "tbl":
			c.table(k)
		case "sdt":
			if sc := k.w("sdtContent"); sc != nil {
				c.blocks(sc)
			}
		case "customXml", "ins", "moveTo":
			c.blocks(k)
		}
	}
}

// ---------- ย่อหน้า ----------
type runItem struct {
	n    *node
	kind int // 1 = text run, 2 = atom
}

// ต้องตรงกับ collectRuns ใน docxpatch.js
func collectRunItems(p *node) []runItem {
	var out []runItem
	var walk func(n *node)
	walk = func(n *node) {
		for _, k := range n.kids {
			if k.space != nsW {
				continue
			}
			switch k.name {
			case "r":
				switch classifyRun(k) {
				case 1:
					out = append(out, runItem{k, 1})
				case 2:
					out = append(out, runItem{k, 2})
				}
			case "hyperlink", "fldSimple", "smartTag", "ins", "moveTo", "customXml", "sdt", "sdtContent", "dir", "bdo":
				walk(k)
			}
		}
	}
	walk(p)
	return out
}

// 0 = มองไม่เห็น/ข้าม, 1 = run ข้อความล้วน (แก้ได้), 2 = มีเนื้อหาที่แก้ไม่ได้ (รูป สัญลักษณ์ ตัวแบ่งหน้า)
func classifyRun(r *node) int {
	hasText, hasAtom := false, false
	for _, k := range r.kids {
		if k.space != nsW {
			if k.name == "AlternateContent" {
				hasAtom = true
			}
			continue
		}
		switch k.name {
		case "t", "tab", "cr":
			hasText = true
		case "br":
			if t := k.attr["type"]; t == "page" || t == "column" {
				hasAtom = true
			} else {
				hasText = true
			}
		case "drawing", "pict", "object", "sym", "noBreakHyphen", "softHyphen", "ptab":
			hasAtom = true
		}
	}
	if hasAtom {
		return 2
	}
	if hasText {
		return 1
	}
	return 0
}

func (c *conv) para(p *node) {
	ppr := p.w("pPr")
	sid := ppr.w("pStyle").val()
	if sid == "" {
		sid = c.st.defParaID
	}
	eff := c.st.defP.over(c.st.ppr(sid, 0))
	direct := parsePPr(ppr)

	// เลขรายการ
	var label string
	var lv *lvl
	if direct.numID.ok || eff.numID.ok {
		nid := eff.numIDStr
		if direct.numID.ok {
			nid = direct.numIDStr
		}
		il := eff.ilvl.or(direct.ilvl)
		if direct.ilvl.ok {
			il = direct.ilvl
		}
		if nid != "" && nid != "0" {
			lv = c.num.level(nid, il.v)
			if lv != nil {
				cnt := c.counters[nid]
				if cnt == nil {
					cnt = &[9]int{}
					c.counters[nid] = cnt
				}
				i := il.v
				if i > 8 {
					i = 8
				}
				if cnt[i] == 0 {
					cnt[i] = lv.start
				} else {
					cnt[i]++
				}
				for j := i + 1; j < 9; j++ {
					cnt[j] = 0
				}
				if lv.fmt == "bullet" {
					label = bulletChar(lv.text)
				} else if lv.fmt != "none" {
					label = lv.text
					for j := 1; j <= 9; j++ {
						ph := "%" + strconv.Itoa(j)
						if strings.Contains(label, ph) {
							f := "decimal"
							if l2 := c.num.level(nid, j-1); l2 != nil {
								f = l2.fmt
							}
							n := cnt[j-1]
							if n == 0 {
								n = 1
							}
							label = strings.ReplaceAll(label, ph, fmtNum(f, n))
						}
					}
				}
				eff.left, eff.first, eff.hang = lv.left, opt{}, lv.hang
			}
		}
	}
	eff = eff.over(direct)
	if lv != nil && !direct.left.ok && !direct.hang.ok && !direct.first.ok {
		// ใช้ indent ของระดับเลขรายการ (ถ้า style ไม่ได้กำหนดเอง)
		eff.left, eff.hang = lv.left, lv.hang
	}

	baseR := c.st.defR.over(c.st.rpr(sid, 0))
	markR := baseR.over(parseRPr(ppr.w("rPr")))

	var css strings.Builder
	if eff.jc != "" {
		switch eff.jc {
		case "center":
			css.WriteString("text-align:center;")
		case "right", "end":
			css.WriteString("text-align:right;")
		case "both", "distribute", "thaiDistribute", "justify":
			css.WriteString("text-align:justify;")
		}
	}
	if eff.left.ok && eff.left.v != 0 {
		fmt.Fprintf(&css, "margin-left:%dpx;", eff.left.v/15)
	}
	if eff.right.ok && eff.right.v != 0 {
		fmt.Fprintf(&css, "margin-right:%dpx;", eff.right.v/15)
	}
	switch {
	case eff.hang.ok && eff.hang.v != 0:
		fmt.Fprintf(&css, "text-indent:-%dpx;", eff.hang.v/15)
	case eff.first.ok && eff.first.v != 0:
		fmt.Fprintf(&css, "text-indent:%dpx;", eff.first.v/15)
	}
	if eff.before.ok && eff.before.v > 0 {
		fmt.Fprintf(&css, "padding-top:%dpx;", eff.before.v/15)
	}
	if eff.after.ok && eff.after.v > 0 {
		fmt.Fprintf(&css, "padding-bottom:%dpx;", eff.after.v/15)
	}
	if eff.line.ok && eff.line.v > 0 {
		if eff.lineRule == "exact" || eff.lineRule == "atLeast" {
			fmt.Fprintf(&css, "line-height:%dpx;", eff.line.v/15)
		} else {
			fmt.Fprintf(&css, "line-height:%.2f;", float64(eff.line.v)/240)
		}
	}
	if eff.shd != "" {
		css.WriteString("background:" + eff.shd + ";")
	}
	if eff.hasBdr {
		for i, s := range sideNames[:4] {
			css.WriteString(eff.bdr[i].css(s))
		}
	}
	sz := markR.sz
	if sz == 0 {
		sz = 22
	}
	fmt.Fprintf(&css, "font-size:%gpt;", float64(sz)/2)
	css.WriteString("font-family:" + fontCSS(firstNonEmpty(markR.fontCS, markR.fontA)) + ";")

	fmt.Fprintf(&c.b, `<div class="p" data-p="%s:%d" style="%s">`, c.part, p.idx, css.String())
	if label != "" {
		w := 0
		if eff.hang.ok {
			w = eff.hang.v / 15
		}
		fmt.Fprintf(&c.b, `<span class="num" contenteditable="false" style="display:inline-block;min-width:%dpx;text-indent:0">%s</span>`, w, htmlEscaper.Replace(label))
	}
	items := collectRunItems(p)
	k, ai := 0, 0
	empty := true
	for _, it := range items {
		if it.kind == 1 {
			c.textRun(it.n, p.idx, k, baseR)
			k++
		} else {
			c.atomRun(it.n, baseR, p.idx, ai)
			ai++
		}
		empty = false
	}
	if empty {
		c.b.WriteString("<br>")
	}
	c.b.WriteString("</div>")
}

func (c *conv) effRun(r *node, base rp) rp {
	eff := base
	rpr := r.w("rPr")
	if rpr != nil {
		if cs := rpr.w("rStyle"); cs != nil {
			eff = eff.over(c.st.rpr(cs.val(), 0))
		}
	}
	return eff.over(parseRPr(rpr))
}

func runStyle(eff rp, text string) string {
	thai := hasThai(text)
	sz, bold, ital := eff.sz, eff.b == 1, eff.i == 1
	font := eff.fontA
	if thai {
		if eff.szcs > 0 {
			sz = eff.szcs
		}
		bold, ital = eff.bcs == 1, eff.ics == 1
		font = firstNonEmpty(eff.fontCS, eff.fontA)
	}
	if sz == 0 {
		sz = 22
	}
	var s strings.Builder
	if eff.vert == "superscript" || eff.vert == "subscript" {
		sz = sz * 2 / 3
		if eff.vert == "superscript" {
			s.WriteString("vertical-align:super;")
		} else {
			s.WriteString("vertical-align:sub;")
		}
	}
	fmt.Fprintf(&s, "font-size:%gpt;font-family:%s;", float64(sz)/2, fontCSS(font))
	if bold {
		s.WriteString("font-weight:700;")
	}
	if ital {
		s.WriteString("font-style:italic;")
	}
	var deco []string
	if eff.u == 1 {
		deco = append(deco, "underline")
	}
	if eff.strike == 1 {
		deco = append(deco, "line-through")
	}
	if len(deco) > 0 {
		s.WriteString("text-decoration:" + strings.Join(deco, " ") + ";")
	}
	if eff.color != "" {
		s.WriteString("color:" + eff.color + ";")
	}
	if eff.hl != "" {
		s.WriteString("background:" + eff.hl + ";")
	}
	if eff.caps == 1 {
		s.WriteString("text-transform:uppercase;")
	}
	return s.String()
}

func (c *conv) writeText(s string) {
	c.b.WriteString(htmlEscaper.Replace(s))
}

func (c *conv) textRun(r *node, pIdx, k int, base rp) {
	var txt strings.Builder
	for _, ch := range r.kids {
		if ch.space != nsW {
			continue
		}
		switch ch.name {
		case "t":
			txt.WriteString(ch.text)
		case "tab":
			txt.WriteString("\t")
		case "br", "cr":
			txt.WriteString("\n")
		}
	}
	text := txt.String()
	eff := c.effRun(r, base)
	fmt.Fprintf(&c.b, `<span data-r="%d.%d" style="%s">`, pIdx, k, runStyle(eff, text))
	var seg strings.Builder
	flush := func() {
		if seg.Len() > 0 {
			c.writeText(seg.String())
			seg.Reset()
		}
	}
	for _, ch := range text {
		switch ch {
		case '\t':
			flush()
			c.b.WriteString(`<span class="tab" data-tab="1">&emsp;&emsp;</span>`)
		case '\n':
			flush()
			c.b.WriteString(`<br data-b="1">`)
		default:
			seg.WriteRune(ch)
		}
	}
	flush()
	c.b.WriteString("</span>")
}

func hasDesc(n *node, name string) bool {
	for _, k := range n.kids {
		if k.name == name || hasDesc(k, name) {
			return true
		}
	}
	return false
}

var wingdings = map[int]string{0xA8: "☐", 0x6F: "☐", 0x71: "☐", 0xA3: "☐", 0xFE: "☑", 0x78: "☒", 0xFD: "☒", 0xFC: "✓", 0xFB: "✗", 0x6E: "■", 0xA7: "▪", 0x6C: "●", 0x76: "❖", 0xD8: "➢"}

func (c *conv) atomRun(r *node, base rp, pIdx, ai int) {
	eff := c.effRun(r, base)
	editableInside := hasDesc(r, "txbxContent")
	attr := fmt.Sprintf(` data-keep="1" data-a="%d.%d"`, pIdx, ai)
	if !editableInside {
		attr += ` contenteditable="false"`
	}
	fmt.Fprintf(&c.b, `<span class="atom"%s style="%s">`, attr, runStyle(eff, "x"))
	for _, ch := range r.kids {
		if ch.space != nsW {
			if ch.name == "AlternateContent" {
				c.drawing(ch)
			}
			continue
		}
		switch ch.name {
		case "t":
			c.writeText(ch.text)
		case "tab":
			c.b.WriteString(`<span class="tab" data-tab="1">&emsp;&emsp;</span>`)
		case "br":
			if t := ch.attr["type"]; t == "page" || t == "column" {
				c.b.WriteString(`<span class="pgbrk">— ขึ้นหน้าใหม่ —</span>`)
			} else {
				c.b.WriteString("<br>")
			}
		case "noBreakHyphen":
			c.b.WriteString("‑")
		case "sym":
			code, _ := strconv.ParseInt(ch.attr["char"], 16, 32)
			low := int(code & 0xFF)
			if g, ok := wingdings[low]; ok && strings.Contains(strings.ToLower(ch.attr["font"]), "wingdings") {
				c.b.WriteString(g)
			} else if code > 0 && code < 0xF000 {
				c.b.WriteString(htmlEscaper.Replace(string(rune(code))))
			} else {
				c.b.WriteString("•")
			}
		case "drawing", "pict", "object":
			c.drawing(ch)
		}
	}
	c.b.WriteString("</span>")
}

// รูป/กล่องข้อความ (DrawingML และ VML)
func (c *conv) drawing(n *node) {
	switch n.name {
	case "AlternateContent":
		for _, k := range n.kids {
			if k.name == "Choice" {
				c.drawing(k)
				return
			}
		}
		for _, k := range n.kids {
			if k.name == "Fallback" {
				c.drawing(k)
				return
			}
		}
		return
	case "txbxContent":
		c.b.WriteString(`<span class="txbx">`)
		c.blocks(n)
		c.b.WriteString(`</span>`)
		return
	case "anchor":
		if n.attr["behindDoc"] == "1" {
			return
		}
	case "blip":
		c.image(n.attr["embed"], 0, 0)
		return
	case "imagedata":
		w, h := vmlSize(n.parent)
		c.image(n.attr["id"], w, h)
		return
	}
	// ขนาดของ DrawingML
	if n.name == "inline" || n.name == "anchor" {
		var cx, cy int
		for _, k := range n.kids {
			if k.name == "extent" {
				cx, cy = atoi(k.attr["cx"])/9525, atoi(k.attr["cy"])/9525
			}
		}
		if cx > 0 {
			for _, k := range n.kids {
				if k.name == "graphic" {
					c.drawingSized(k, cx, cy)
					return
				}
			}
		}
	}
	for _, k := range n.kids {
		c.drawing(k)
	}
}

func (c *conv) drawingSized(n *node, w, h int) {
	if n.name == "blip" {
		c.image(n.attr["embed"], w, h)
		return
	}
	if n.name == "txbxContent" {
		c.drawing(n)
		return
	}
	for _, k := range n.kids {
		c.drawingSized(k, w, h)
	}
}

func vmlSize(n *node) (int, int) {
	if n == nil {
		return 0, 0
	}
	w, h := 0, 0
	for _, part := range strings.Split(n.attr["style"], ";") {
		kv := strings.SplitN(part, ":", 2)
		if len(kv) != 2 {
			continue
		}
		v := strings.TrimSpace(kv[1])
		px := 0
		switch {
		case strings.HasSuffix(v, "pt"):
			f, _ := strconv.ParseFloat(strings.TrimSuffix(v, "pt"), 64)
			px = int(f * 4 / 3)
		case strings.HasSuffix(v, "px"):
			f, _ := strconv.ParseFloat(strings.TrimSuffix(v, "px"), 64)
			px = int(f)
		}
		switch strings.TrimSpace(kv[0]) {
		case "width":
			w = px
		case "height":
			h = px
		}
	}
	return w, h
}

func (c *conv) image(rid string, w, h int) {
	target := c.rels[rid]
	data := c.files[target]
	if target == "" || data == nil || len(data) > 4<<20 {
		return
	}
	mime := "image/png"
	switch strings.ToLower(path.Ext(target)) {
	case ".jpg", ".jpeg":
		mime = "image/jpeg"
	case ".gif":
		mime = "image/gif"
	case ".bmp":
		mime = "image/bmp"
	case ".svg":
		mime = "image/svg+xml"
	case ".emf", ".wmf", ".tif", ".tiff":
		return // เบราว์เซอร์แสดงไม่ได้
	}
	size := ""
	if w > 0 && h > 0 {
		size = fmt.Sprintf(` width="%d" height="%d"`, w, h)
	}
	fmt.Fprintf(&c.b, `<img src="data:%s;base64,%s"%s style="max-width:100%%">`, mime, base64.StdEncoding.EncodeToString(data), size)
}

// ---------- ตาราง ----------
type cellInfo struct {
	n       *node
	col     int
	span    int
	rowspan int
	skip    bool
}

func (c *conv) table(tbl *node) {
	tblPr := tbl.w("tblPr")
	var widths []int
	if g := tbl.w("tblGrid"); g != nil {
		for _, gc := range g.kids {
			if gc.name == "gridCol" {
				widths = append(widths, atoi(gc.attr["w"]))
			}
		}
	}
	// ขอบตาราง: ของตารางเองทับของ style
	var tb [6]border
	if sid := tblPr.w("tblStyle").val(); sid != "" {
		tb, _ = c.st.tblBorders(sid, 0)
	} else if c.st.defTableID != "" {
		tb, _ = c.st.tblBorders(c.st.defTableID, 0)
	}
	if b := tblPr.w("tblBorders"); b != nil {
		own := parseBorders(b)
		for i := range tb {
			if own[i].set {
				tb[i] = own[i]
			}
		}
	}

	// สร้างโมเดลแถว/เซลล์ + คำนวณ rowspan จาก vMerge
	var rows [][]*cellInfo
	var trs []*node
	active := map[int]*cellInfo{}
	ncols := len(widths)
	for _, tr := range tbl.kids {
		if !tr.is("tr") {
			continue
		}
		trs = append(trs, tr)
		col := 0
		if trPr := tr.w("trPr"); trPr != nil {
			col += atoi(trPr.w("gridBefore").val())
		}
		var row []*cellInfo
		for _, tc := range tr.kids {
			if !tc.is("tc") {
				continue
			}
			tcPr := tc.w("tcPr")
			span := 1
			if gs := tcPr.w("gridSpan"); gs != nil && atoi(gs.val()) > 1 {
				span = atoi(gs.val())
			}
			ci := &cellInfo{n: tc, col: col, span: span, rowspan: 1}
			if vm := tcPr.w("vMerge"); vm != nil {
				if vm.val() == "restart" {
					active[col] = ci
				} else if st := active[col]; st != nil {
					st.rowspan++
					ci.skip = true
				}
			} else {
				delete(active, col)
			}
			row = append(row, ci)
			col += span
		}
		if col > ncols {
			ncols = col
		}
		rows = append(rows, row)
	}

	sum := 0
	for _, w := range widths {
		sum += w
	}
	tblW := sum
	if w := tblPr.w("tblW"); w != nil && w.attr["type"] == "dxa" && atoi(w.attr["w"]) > 0 {
		tblW = atoi(w.attr["w"])
	}
	var tcss strings.Builder
	if tblW > 0 {
		fmt.Fprintf(&tcss, "width:%dpx;max-width:100%%;", tblW/15)
	}
	switch tblPr.w("jc").val() {
	case "center":
		tcss.WriteString("margin-left:auto;margin-right:auto;")
	case "right", "end":
		tcss.WriteString("margin-left:auto;")
	default:
		if ind := tblPr.w("tblInd"); ind != nil && atoi(ind.attr["w"]) > 0 {
			fmt.Fprintf(&tcss, "margin-left:%dpx;", atoi(ind.attr["w"])/15)
		}
	}
	fmt.Fprintf(&c.b, `<table class="docx-tbl" style="%s">`, tcss.String())
	if sum > 0 {
		c.b.WriteString("<colgroup>")
		for _, w := range widths {
			fmt.Fprintf(&c.b, `<col style="width:%.2f%%">`, float64(w)*100/float64(sum))
		}
		c.b.WriteString("</colgroup>")
	}
	c.b.WriteString("<tbody>")
	for ri, tr := range trs {
		style := ""
		if h := tr.w("trPr").w("trHeight"); h != nil && atoi(h.attr["val"]) > 0 {
			style = fmt.Sprintf(` style="height:%dpx"`, atoi(h.attr["val"])/15)
		}
		fmt.Fprintf(&c.b, `<tr data-tr="%s:%d"%s>`, c.part, tr.idx, style)
		for _, ci := range rows[ri] {
			if ci.skip {
				continue
			}
			c.cell(ci, tb, ri, len(trs), ncols)
		}
		c.b.WriteString("</tr>")
	}
	c.b.WriteString("</tbody></table>")
}

func (c *conv) cell(ci *cellInfo, tb [6]border, ri, nrows, ncols int) {
	tcPr := ci.n.w("tcPr")
	var css strings.Builder
	own := parseBorders(tcPr.w("tcBorders"))
	top, bottom := tb[4], tb[4]
	if ri == 0 {
		top = tb[0]
	}
	if ri+ci.rowspan >= nrows {
		bottom = tb[2]
	}
	left, right := tb[5], tb[5]
	if ci.col == 0 {
		left = tb[1]
	}
	if ci.col+ci.span >= ncols {
		right = tb[3]
	}
	pick := func(o, t border) border {
		if o.set {
			return o
		}
		return t
	}
	css.WriteString(pick(own[0], top).css("top"))
	css.WriteString(pick(own[1], left).css("left"))
	css.WriteString(pick(own[2], bottom).css("bottom"))
	css.WriteString(pick(own[3], right).css("right"))
	if shd := tcPr.w("shd"); shd != nil {
		if f := shd.attr["fill"]; f != "" && f != "auto" {
			css.WriteString("background:#" + f + ";")
		}
	}
	switch tcPr.w("vAlign").val() {
	case "center":
		css.WriteString("vertical-align:middle;")
	case "bottom":
		css.WriteString("vertical-align:bottom;")
	default:
		css.WriteString("vertical-align:top;")
	}
	attrs := ""
	if ci.span > 1 {
		attrs += fmt.Sprintf(` colspan="%d"`, ci.span)
	}
	if ci.rowspan > 1 {
		attrs += fmt.Sprintf(` rowspan="%d"`, ci.rowspan)
	}
	fmt.Fprintf(&c.b, `<td%s style="%s">`, attrs, css.String())
	c.blocks(ci.n)
	c.b.WriteString("</td>")
}
