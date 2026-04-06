#!/usr/bin/env python3

import copy
import json
from pathlib import Path
import tkinter as tk
from tkinter import colorchooser, messagebox, simpledialog, ttk


ROOT = Path(__file__).resolve().parent.parent
CATALOG_PATH = ROOT / "assets" / "themes" / "presets.json"

THEME_FIELDS = [
    "app_background",
    "panel_background",
    "panel_background_alt",
    "overlay_background",
    "border",
    "border_strong",
    "border_soft",
    "text_primary",
    "text_secondary",
    "text_muted",
    "text_soft",
    "button_background",
    "button_border",
    "button_text",
    "tab_shell_background",
    "tab_shell_border",
    "info_accent",
    "info_background",
    "info_text",
    "success_text",
    "warning_background",
    "warning_border",
    "warning_text",
    "error_background",
    "error_border",
    "error_text",
    "notification_background",
    "notification_border",
    "notification_text",
]

TAB_IDS = [
    "state",
    "connection-status",
    "detailed",
    "map",
    "actions",
    "calibration",
    "notifications",
    "warnings",
    "errors",
    "data",
    "network-topology",
]


def load_catalog():
    return json.loads(CATALOG_PATH.read_text(encoding="utf-8"))


def save_catalog(catalog):
    CATALOG_PATH.write_text(json.dumps(catalog, indent=2) + "\n", encoding="utf-8")


class ThemeEditor(tk.Tk):
    def __init__(self):
        super().__init__()
        self.title("GS26 Theme Editor")
        self.geometry("1280x860")
        self.minsize(1100, 760)
        self.catalog = load_catalog()
        self.current_index = None

        self.id_var = tk.StringVar()
        self.label_vars = {lang: tk.StringVar() for lang in ("en", "es", "fr")}
        self.theme_vars = {field: tk.StringVar() for field in THEME_FIELDS}
        self.accent_vars = {tab_id: tk.StringVar() for tab_id in TAB_IDS}
        self.status_var = tk.StringVar(value=f"Loaded {CATALOG_PATH}")

        self._build_ui()
        self._reload_list()
        if self.catalog["presets"]:
            self._select_index(0)

    def _build_ui(self):
        self.columnconfigure(1, weight=1)
        self.rowconfigure(0, weight=1)

        left = ttk.Frame(self, padding=12)
        left.grid(row=0, column=0, sticky="ns")
        left.rowconfigure(1, weight=1)

        ttk.Label(left, text="Theme Presets").grid(row=0, column=0, sticky="w")
        self.listbox = tk.Listbox(left, width=28, exportselection=False)
        self.listbox.grid(row=1, column=0, sticky="ns")
        self.listbox.bind("<<ListboxSelect>>", self._on_select)

        button_bar = ttk.Frame(left)
        button_bar.grid(row=2, column=0, sticky="ew", pady=(8, 0))
        for idx, (label, command) in enumerate(
            [
                ("New", self._new_preset),
                ("Clone", self._clone_preset),
                ("Delete", self._delete_preset),
                ("Reload", self._reload_from_disk),
                ("Save", self._save),
            ]
        ):
            ttk.Button(button_bar, text=label, command=command).grid(
                row=idx, column=0, sticky="ew", pady=2
            )

        editor_wrap = ttk.Frame(self, padding=(0, 12, 12, 12))
        editor_wrap.grid(row=0, column=1, sticky="nsew")
        editor_wrap.columnconfigure(0, weight=1)
        editor_wrap.rowconfigure(0, weight=1)

        canvas = tk.Canvas(editor_wrap, highlightthickness=0)
        scrollbar = ttk.Scrollbar(
            editor_wrap, orient="vertical", command=canvas.yview
        )
        canvas.grid(row=0, column=0, sticky="nsew")
        scrollbar.grid(row=0, column=1, sticky="ns")
        canvas.configure(yscrollcommand=scrollbar.set)

        inner = ttk.Frame(canvas, padding=4)
        window_id = canvas.create_window((0, 0), window=inner, anchor="nw")
        inner.bind(
            "<Configure>",
            lambda _event: canvas.configure(scrollregion=canvas.bbox("all")),
        )
        canvas.bind(
            "<Configure>",
            lambda event: canvas.itemconfigure(window_id, width=event.width),
        )

        self._build_form(inner)
        ttk.Label(
            self, textvariable=self.status_var, anchor="w", padding=(12, 0, 12, 8)
        ).grid(row=1, column=0, columnspan=2, sticky="ew")

    def _build_form(self, parent):
        parent.columnconfigure(1, weight=1)

        row = 0
        ttk.Label(parent, text="Preset Id").grid(row=row, column=0, sticky="w", pady=4)
        ttk.Entry(parent, textvariable=self.id_var).grid(
            row=row, column=1, sticky="ew", pady=4
        )

        row += 1
        ttk.Separator(parent, orient="horizontal").grid(
            row=row, column=0, columnspan=3, sticky="ew", pady=8
        )
        row += 1
        ttk.Label(parent, text="Localized Labels").grid(
            row=row, column=0, sticky="w", pady=(0, 6)
        )

        for lang in ("en", "es", "fr"):
            row += 1
            ttk.Label(parent, text=lang.upper()).grid(row=row, column=0, sticky="w", pady=3)
            ttk.Entry(parent, textvariable=self.label_vars[lang]).grid(
                row=row, column=1, sticky="ew", pady=3
            )

        row += 1
        ttk.Separator(parent, orient="horizontal").grid(
            row=row, column=0, columnspan=3, sticky="ew", pady=8
        )

        row += 1
        ttk.Label(parent, text="Theme Colors").grid(
            row=row, column=0, sticky="w", pady=(0, 6)
        )
        for field in THEME_FIELDS:
            row += 1
            self._color_field(parent, row, field, self.theme_vars[field])

        row += 1
        ttk.Separator(parent, orient="horizontal").grid(
            row=row, column=0, columnspan=3, sticky="ew", pady=8
        )

        row += 1
        ttk.Label(parent, text="Main Tab Accents").grid(
            row=row, column=0, sticky="w", pady=(0, 6)
        )
        for tab_id in TAB_IDS:
            row += 1
            self._color_field(parent, row, tab_id, self.accent_vars[tab_id])

    def _color_field(self, parent, row, label, variable):
        ttk.Label(parent, text=label).grid(row=row, column=0, sticky="w", pady=2)
        ttk.Entry(parent, textvariable=variable).grid(
            row=row, column=1, sticky="ew", pady=2
        )
        ttk.Button(
            parent,
            text="Pick",
            command=lambda v=variable: self._pick_color(v),
            width=7,
        ).grid(row=row, column=2, sticky="e", padx=(8, 0), pady=2)

    def _pick_color(self, variable):
        color = colorchooser.askcolor(color=variable.get() or "#ffffff", parent=self)[1]
        if color:
            variable.set(color.lower())

    def _reload_list(self):
        self.listbox.delete(0, tk.END)
        for preset in self.catalog.get("presets", []):
            self.listbox.insert(tk.END, preset["id"])

    def _on_select(self, _event=None):
        selection = self.listbox.curselection()
        if not selection:
            return
        self._select_index(selection[0])

    def _select_index(self, index):
        presets = self.catalog.get("presets", [])
        if index < 0 or index >= len(presets):
            return
        self.current_index = index
        preset = presets[index]
        self.id_var.set(preset.get("id", ""))
        labels = preset.get("label", {})
        for lang, variable in self.label_vars.items():
            variable.set(labels.get(lang, ""))
        theme = preset.get("theme", {})
        for field, variable in self.theme_vars.items():
            variable.set(theme.get(field, ""))
        accents = theme.get("main_tab_accents", {})
        for tab_id, variable in self.accent_vars.items():
            variable.set(accents.get(tab_id, ""))
        self.listbox.selection_clear(0, tk.END)
        self.listbox.selection_set(index)
        self.listbox.activate(index)
        self.status_var.set(f"Editing preset '{preset['id']}'")

    def _gather_current(self):
        preset_id = self.id_var.get().strip()
        if not preset_id:
            raise ValueError("Preset id is required.")
        theme = {field: self.theme_vars[field].get().strip() for field in THEME_FIELDS}
        theme["main_tab_accents"] = {
            tab_id: self.accent_vars[tab_id].get().strip() for tab_id in TAB_IDS
        }
        return {
            "id": preset_id,
            "label": {lang: var.get().strip() for lang, var in self.label_vars.items()},
            "theme": theme,
        }

    def _apply_current_to_catalog(self):
        if self.current_index is None:
            raise ValueError("No preset selected.")
        gathered = self._gather_current()
        presets = self.catalog["presets"]
        duplicate_index = next(
            (
                idx
                for idx, preset in enumerate(presets)
                if idx != self.current_index and preset["id"] == gathered["id"]
            ),
            None,
        )
        if duplicate_index is not None:
            raise ValueError(f"Preset id '{gathered['id']}' already exists.")
        presets[self.current_index] = gathered
        self._reload_list()
        self._select_index(self.current_index)

    def _new_preset(self):
        base_id = simpledialog.askstring("New Preset", "Preset id:", parent=self)
        if not base_id:
            return
        preset_id = base_id.strip()
        if any(p["id"] == preset_id for p in self.catalog["presets"]):
            messagebox.showerror("Theme Editor", f"Preset '{preset_id}' already exists.")
            return
        template = (
            copy.deepcopy(self.catalog["presets"][self.current_index])
            if self.current_index is not None
            else {
                "id": preset_id,
                "label": {"en": "", "es": "", "fr": ""},
                "theme": {
                    **{field: "" for field in THEME_FIELDS},
                    "main_tab_accents": {tab_id: "" for tab_id in TAB_IDS},
                },
            }
        )
        template["id"] = preset_id
        self.catalog["presets"].append(template)
        self._reload_list()
        self._select_index(len(self.catalog["presets"]) - 1)

    def _clone_preset(self):
        if self.current_index is None:
            return
        source = self.catalog["presets"][self.current_index]
        clone_id = simpledialog.askstring(
            "Clone Preset", "New preset id:", initialvalue=f"{source['id']}_copy", parent=self
        )
        if not clone_id:
            return
        clone_id = clone_id.strip()
        if any(p["id"] == clone_id for p in self.catalog["presets"]):
            messagebox.showerror("Theme Editor", f"Preset '{clone_id}' already exists.")
            return
        clone = copy.deepcopy(source)
        clone["id"] = clone_id
        self.catalog["presets"].append(clone)
        self._reload_list()
        self._select_index(len(self.catalog["presets"]) - 1)

    def _delete_preset(self):
        if self.current_index is None:
            return
        preset = self.catalog["presets"][self.current_index]
        if not messagebox.askyesno(
            "Delete Preset",
            f"Delete preset '{preset['id']}' from {CATALOG_PATH.name}?",
            parent=self,
        ):
            return
        del self.catalog["presets"][self.current_index]
        self._reload_list()
        if self.catalog["presets"]:
            self._select_index(max(0, self.current_index - 1))
        else:
            self.current_index = None

    def _reload_from_disk(self):
        if not messagebox.askyesno(
            "Reload",
            "Discard unsaved changes and reload the theme catalog from disk?",
            parent=self,
        ):
            return
        self.catalog = load_catalog()
        self._reload_list()
        if self.catalog["presets"]:
            self._select_index(0)

    def _save(self):
        try:
            self._apply_current_to_catalog()
            save_catalog(self.catalog)
        except Exception as exc:
            messagebox.showerror("Theme Editor", str(exc), parent=self)
            return
        self.status_var.set(f"Saved {CATALOG_PATH}")


if __name__ == "__main__":
    ThemeEditor().mainloop()
