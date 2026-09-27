// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! asqr puts questions that another program has prepared, mainly an AI
//! coding agent, in front of a person in the terminal and hands the answers
//! back through files.
//!
//! The library holds all of asqr's logic, so tests and later front ends
//! reach it without a terminal. The `asqr` binary only parses arguments and
//! drives the terminal (ADR 16).
