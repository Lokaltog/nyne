(local lume (require :lume))
(local utils (require :utils))

(local MAX-RETRIES 3)

(fn greet [name]
  "Greet someone by name."
  (.. "Hello, " name "!"))

(lambda process [input output]
  "Process input and write to output."
  (output (input:upper)))

(macro with-retry [n ...]
  `(for [_# 1 ,n]
     ,...))

(local config {:name "default"
               :debug false})
