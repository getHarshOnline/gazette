"""Owned, dependency-free autodoc qualification; no third-party extensions."""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
project = "Gazette Python fixture"
version = "0.1"
release = "0.1.0"
extensions = ["sphinx.ext.autodoc"]
root_doc = "index"
html_theme = "basic"
highlight_language = "none"
html_copy_source = True
html_show_sourcelink = True
autodoc_typehints = "signature"
autodoc_member_order = "bysource"
exclude_patterns = []
