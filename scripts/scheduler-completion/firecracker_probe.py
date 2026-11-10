#!/usr/bin/env python3
"""ROOT-only actual VFIO host-to-guest PCI witness from the selected VMM's API."""

import argparse
import http.client
import json
import re
import socket
import sys
import time
from pathlib import Path

from evidence import InvalidEvidence, require


def pci_tuple(text):
    require(isinstance(text,str) and re.fullmatch(r"[0-9a-f]{4,8}:[0-9a-f]{2}:[0-1][0-9a-f]\.[0-7]",text),
            "noncanonical native PCI address")
    domain,bus,tail=text.split(":")
    device,function=tail.split(".")
    return [int(domain,16),int(bus,16),int(device,16),int(function,16)]


def key(pci):
    domain,bus,device,function=pci
    return (domain<<16)|(bus<<8)|(device<<3)|function


def witness(api, host_sbdf, expected_key, timeout, byte_limit):
    require(key(pci_tuple(host_sbdf))==expected_key,"selected native key differs from actual host PCI")
    identifier="pci-"+host_sbdf
    deadline=time.monotonic()+timeout
    connection=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)
    try:
        connection.settimeout(timeout)
        connection.connect(str(api))
        request=f"GET /device-passthrough/{identifier} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
        connection.sendall(request.encode("ascii"))
        response=http.client.HTTPResponse(connection)
        connection.settimeout(max(0,deadline-time.monotonic()))
        response.begin()
        require(response.status==200,"actual VMM refused physical attachment witness")
        raw=response.read(byte_limit+1)
        require(len(raw)<=byte_limit,"actual attachment response exceeded admitted bound")
        row=json.loads(raw)
        require(row.get("sbdf")==host_sbdf or row.get("host_sbdf")==host_sbdf,
                "VMM witness belongs to another physical host device")
        guest=pci_tuple(row["guest_sbdf"])
        return {"native_key":expected_key,"host_pci":pci_tuple(host_sbdf),"guest_pci":guest,
                "api":str(api),"vmm_response":row}
    finally:
        connection.close()


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root-authorized",action="store_true")
    parser.add_argument("--api",type=Path,required=True)
    parser.add_argument("--host-sbdf",required=True)
    parser.add_argument("--native-key",type=int,required=True)
    parser.add_argument("--timeout",type=float,required=True)
    parser.add_argument("--byte-limit",type=int,required=True)
    args=parser.parse_args()
    require(args.root_authorized and args.timeout>0 and args.byte_limit>0,"ROOT finite envelope required")
    print(json.dumps(witness(args.api,args.host_sbdf,args.native_key,args.timeout,args.byte_limit)))


if __name__=="__main__":
    try:
        main()
    except (InvalidEvidence,OSError,ValueError,KeyError,http.client.HTTPException) as error:
        print(f"UNVERIFIED Firecracker physical join: {error}",file=sys.stderr)
        sys.exit(1)
