#!/usr/bin/env python3

import argparse
import filecmp
import importlib
import inspect
import subprocess
import sys
import tempfile

import img
from harness import TestCase

def run_cmd_with_io(cmd, input, output):
    assert cmd is not None
    pack_cmd = cmd.format(input = input, output = output).split(' ')
    proc = subprocess.run(pack_cmd, capture_output=True)
    if proc.returncode != 0:
        raise Exception('Failed to run command')

def run_test_case(case, args):
    print('Running test case:', case.name)
    
    dir = tempfile.TemporaryDirectory()
    case.set_up(dir.name)

    output_img = tempfile.NamedTemporaryFile()
    run_cmd_with_io(args.pack_cmd, dir.name, output_img.name)

    output_dir = tempfile.TemporaryDirectory()
    run_cmd_with_io(args.unpack_cmd, output_img.name, output_dir.name)

    l1_cmp = filecmp.dircmp(dir.name, output_dir.name)
    if l1_cmp.diff_files or l1_cmp.left_only or l1_cmp.right_only or l1_cmp.funny_files:
        l1_cmp.report_full_closure()
        raise Exception('Mismatch between test case input and packed output')
    
    repack_img = tempfile.NamedTemporaryFile()
    repack_output = tempfile.TemporaryDirectory()
    run_cmd_with_io(args.repack_cmd, output_img.name, repack_img.name)
    run_cmd_with_io(args.unpack_cmd, repack_img.name, repack_output.name)

    l2_cmp = filecmp.dircmp(dir.name, repack_output.name)
    if l2_cmp.diff_files or l2_cmp.left_only or l2_cmp.right_only or l2_cmp.funny_files:
        l2_cmp.report_full_closure()
        raise Exception('Mismatch between test case input and repacked output')

def run_test_case_module(module, args):
    module_contents = [getattr(module, c) for c in dir(module)]
    test_cases = filter(lambda c: inspect.isclass(c) and issubclass(c, TestCase), module_contents)

    result = {}
    for case in test_cases:
        case_inst = case()
        try:
            run_test_case(case_inst, args)
            result[case_inst.name] = True
        except Exception as e:
            print(e)
            result[case_inst.name] = False

    return result

def run_tests(args):
    result = {}
    result.update(run_test_case_module(img, args))

    return result

def report_results(results):
    print('Results:')

    all_ok = True
    for k, v in result.items():
        print('{:<32}: {:<4}'.format(k, 'PASS' if v else 'FAIL'))
        if not v:
            all_ok = False

    return all_ok

def parse_args():
    parser = argparse.ArgumentParser(description='XDVDFS test suite')

    parser.add_argument('-p', '--pack', dest='pack_cmd', default='cargo r --bin xdvdfs -- pack {input} {output}', help='Command to pack a directory. Use "{input}" and "{output}" as placeholders for input and output paths')
    parser.add_argument('-u', '--unpack', dest='unpack_cmd', default='cargo r --bin xdvdfs -- unpack {input} {output}', help='Command to unpack an image. Use "{input}" and "{output}" as placeholders for input and output paths')
    parser.add_argument('-r', '--repack', dest='repack_cmd', default='cargo r --bin xdvdfs -- pack {input} {output}', help='Command to repack an image. Use "{input}" and "{output}" as placeholders for input and output paths')

    return parser.parse_args()

if __name__ == '__main__':
    args = parse_args()
    result = run_tests(args)
    print()

    all_ok = report_results(result)
    if not all_ok:
        sys.exit(1)
