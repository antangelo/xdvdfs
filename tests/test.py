#!/usr/bin/env python3

import argparse
import inspect
import sys
import tempfile

import img
from harness import TestCase

CMD_PACK = 'cargo r --bin xdvdfs -- pack {input} {output}'
CMD_UNPACK = 'cargo r --bin xdvdfs -- unpack {input} {output}'
CMD_REPACK = 'cargo r --bin xdvdfs -- pack {input} {output}'

def run_test_case(case, args):
    print('Running test case:', case.name)
    
    dir = tempfile.TemporaryDirectory()
    case.set_up(dir.name)
    case.run(dir.name, args)

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

    parser.add_argument('-p', '--pack', dest='pack_cmd', default=CMD_PACK, help='Command to pack a directory. Use "{input}" and "{output}" as placeholders for input and output paths')
    parser.add_argument('-u', '--unpack', dest='unpack_cmd', default=CMD_UNPACK, help='Command to unpack an image. Use "{input}" and "{output}" as placeholders for input and output paths')
    parser.add_argument('-r', '--repack', dest='repack_cmd', default=CMD_REPACK, help='Command to repack an image. Use "{input}" and "{output}" as placeholders for input and output paths')

    return parser.parse_args()

if __name__ == '__main__':
    args = parse_args()
    result = run_tests(args)
    print()

    all_ok = report_results(result)
    if not all_ok:
        sys.exit(1)
